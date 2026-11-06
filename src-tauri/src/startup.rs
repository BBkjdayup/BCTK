//! Startup runs away from the window thread. Progress never acquires a database
//! lease, so the UI can observe maintenance before AppState is published.
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::watch;

use crate::{api::AppState, data_move, restore, uninstall};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSnapshot {
    pub phase: String,
    pub message: String,
    pub completed: Option<usize>,
    pub total: Option<usize>,
    pub elapsed_ms: u64,
    pub finished: bool,
    pub error: Option<String>,
    pub log_path: Option<String>,
}

struct ProgressData {
    snapshot: StartupSnapshot,
    started: Instant,
    last_log: Instant,
}

#[derive(Clone)]
pub struct StartupProgress {
    data: Arc<Mutex<ProgressData>>,
    log: Arc<Mutex<Option<File>>>,
}

impl Default for StartupProgress {
    fn default() -> Self {
        Self {
            data: Arc::new(Mutex::new(ProgressData {
                snapshot: StartupSnapshot {
                    phase: "starting".into(),
                    message: "正在准备题库".into(),
                    completed: None,
                    total: None,
                    elapsed_ms: 0,
                    finished: false,
                    error: None,
                    log_path: None,
                },
                started: Instant::now(),
                last_log: Instant::now(),
            })),
            log: Arc::new(Mutex::new(None)),
        }
    }
}

impl StartupProgress {
    /// Called only on the startup worker. Keep the previous run, without ever
    /// putting question content or credentials into these diagnostics.
    pub fn start_logging(&self, app_local_root: &Path, app_version: &str) {
        let logs = app_local_root.join("logs");
        let path = logs.join("startup-latest.jsonl");
        let opened = (|| -> std::io::Result<File> {
            fs::create_dir_all(&logs)?;
            if path.is_file() {
                fs::copy(&path, logs.join("startup-previous.jsonl"))?;
            }
            File::create(&path)
        })();
        if let Ok(mut file) = opened {
            let _ = writeln!(file, "{}", serde_json::json!({"appVersion": app_version}));
            *self.log.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
            self.data
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .snapshot
                .log_path = Some(path.to_string_lossy().into_owned());
        }
    }

    pub fn snapshot(&self) -> StartupSnapshot {
        let data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let mut snapshot = data.snapshot.clone();
        if !snapshot.finished {
            snapshot.elapsed_ms = data.started.elapsed().as_millis() as u64;
        }
        snapshot
    }

    pub fn restart(&self) {
        {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            data.started = Instant::now();
            data.snapshot.finished = false;
            data.snapshot.error = None;
        }
        self.stage("starting", "正在重新准备题库");
    }

    pub fn stage(&self, phase: &str, message: &str) {
        self.update(phase, message, None, None, true);
    }

    pub fn rebuilding(&self, completed: usize, total: usize) {
        self.update(
            "rebuilding",
            "正在更新旧题库的搜索和查重信息",
            Some(completed),
            Some(total),
            completed == total,
        );
    }

    fn update(
        &self,
        phase: &str,
        message: &str,
        completed: Option<usize>,
        total: Option<usize>,
        force_log: bool,
    ) {
        let should_log = {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            let should_log = force_log
                || data.snapshot.phase != phase
                || data.last_log.elapsed() >= Duration::from_secs(1);
            data.snapshot.phase = phase.into();
            data.snapshot.message = message.into();
            data.snapshot.completed = completed;
            data.snapshot.total = total;
            data.snapshot.elapsed_ms = data.started.elapsed().as_millis() as u64;
            if should_log {
                data.last_log = Instant::now();
            }
            should_log
        };
        if should_log {
            self.write_log();
        }
    }

    pub fn finish(&self, error: Option<String>) {
        {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            data.snapshot.finished = true;
            data.snapshot.elapsed_ms = data.started.elapsed().as_millis() as u64;
            data.snapshot.phase = if error.is_some() { "failed" } else { "ready" }.into();
            data.snapshot.message = if error.is_some() {
                "题库加载未完成"
            } else {
                "题库已准备好"
            }
            .into();
            data.snapshot.error = error;
        }
        self.write_log();
    }

    fn write_log(&self) {
        let snapshot = self.snapshot();
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        if let Some(file) = self.log.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            // A diagnostic write failure must never stop a database upgrade.
            let _ = writeln!(
                file,
                "{}",
                serde_json::json!({"timestampMs": timestamp_ms, "startup": snapshot})
            );
        }
    }
}

pub struct StartupState {
    pub progress: StartupProgress,
    completion: watch::Sender<Option<Result<(), String>>>,
}

impl Default for StartupState {
    fn default() -> Self {
        Self {
            progress: StartupProgress::default(),
            completion: watch::channel(None).0,
        }
    }
}

impl StartupState {
    pub async fn wait_ready(&self) -> Result<(), String> {
        let mut receiver = self.completion.subscribe();
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| "启动任务已结束，请重新打开软件。".to_owned())?;
        }
    }

    /// Publish AppState before waking any bootstrap request, even for an
    /// unavailable database: that state supplies the existing rescue/retry UI.
    pub fn registered(&self, database_error: Option<String>) {
        self.progress.finish(database_error);
        self.completion.send_replace(Some(Ok(())));
    }

    pub fn failed(&self, message: String) {
        self.progress.finish(Some(message.clone()));
        self.completion.send_replace(Some(Err(message)));
    }
}

pub(crate) async fn initialize_data_root(
    data_root: PathBuf,
    app_version: String,
    progress: &StartupProgress,
) -> AppState {
    progress.stage("restore", "正在检查上次恢复任务");
    match restore::apply_pending_restore(&data_root).await {
        Ok(_) => AppState::initialize_with_progress(data_root, app_version, progress).await,
        Err(error) => AppState::unavailable(
            data_root,
            app_version,
            format!("恢复维护未完成，数据库保持关闭：{}", error.message),
        ),
    }
}

pub async fn initialize(
    default_data_root: PathBuf,
    app_local_root: PathBuf,
    app_version: String,
    progress: &StartupProgress,
) -> Result<AppState, String> {
    progress.start_logging(&app_local_root, &app_version);
    progress.stage("location", "正在检查题库位置");
    let bootstrap_dir = app_local_root.join("bootstrap");
    let startup =
        match data_move::resolve_startup_data_root(&bootstrap_dir, default_data_root.clone()) {
            Ok(startup) => startup,
            Err(error) => {
                return Err(format!(
                    "题库位置无法确认，请检查目录配置后重新打开软件：{}",
                    error.message
                ));
            }
        };
    progress.stage("location_validation", "正在验证题库目录");
    let mut selected_state = match data_move::validate_pending_target(&startup).await {
        Ok(()) => {
            initialize_data_root(
                startup.selected_data_root.clone(),
                app_version.clone(),
                progress,
            )
            .await
        }
        Err(error) => AppState::unavailable(
            startup.selected_data_root.clone(),
            app_version.clone(),
            error.message,
        ),
    };
    let selected_error = selected_state.initialization_error().await;
    progress.stage("location_commit", "正在确认题库位置");
    match data_move::finalize_pending_startup(&bootstrap_dir, &startup, selected_error) {
        Ok(Some(result)) if result.outcome == "rolled_back" => {
            let fallback = PathBuf::from(result.active_data_root);
            selected_state = match data_move::validate_existing_data_root(&fallback) {
                Ok(()) => initialize_data_root(fallback, app_version.clone(), progress).await,
                Err(error) => AppState::unavailable(
                    fallback,
                    app_version.clone(),
                    format!(
                        "迁移目标未启用，但原数据目录也无法安全打开：{}",
                        error.message
                    ),
                ),
            };
        }
        Ok(_) => {}
        Err(error) => {
            selected_state = AppState::unavailable(
                startup.selected_data_root,
                app_version,
                format!("数据目录切换状态无法安全提交：{}", error.message),
            )
        }
    }
    if let Err(error) =
        uninstall::register_managed_data_root(&app_local_root, selected_state.data_root())
    {
        eprintln!("无法登记安全卸载数据目录：{error}");
    }
    Ok(selected_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bootstrap_waits_without_blocking_progress_and_handles_late_subscribers() {
        let startup = Arc::new(StartupState::default());
        let waiting = Arc::clone(&startup);
        let task = tokio::spawn(async move { waiting.wait_ready().await });
        tokio::task::yield_now().await;
        startup.progress.rebuilding(64, 1000);
        assert_eq!(startup.progress.snapshot().completed, Some(64));
        assert!(!task.is_finished());
        startup.registered(Some("synthetic database error".into()));
        assert!(task.await.unwrap().is_ok());
        assert!(startup.wait_ready().await.is_ok());
        assert_eq!(startup.progress.snapshot().phase, "failed");
    }

    #[tokio::test]
    async fn worker_failure_wakes_bootstrap_with_error() {
        let startup = StartupState::default();
        startup.failed("synthetic worker failure".into());
        assert_eq!(
            startup.wait_ready().await.unwrap_err(),
            "synthetic worker failure"
        );
    }

    #[tokio::test]
    async fn invalid_location_does_not_publish_a_retryable_default_database() {
        let root =
            std::env::temp_dir().join(format!("tk-startup-location-{}", uuid::Uuid::now_v7()));
        let app_local = root.join("app-local");
        fs::create_dir_all(&app_local).unwrap();
        fs::write(app_local.join("bootstrap"), b"not a directory").unwrap();
        let default_data = root.join("default-data");
        let progress = StartupProgress::default();
        let result = initialize(default_data.clone(), app_local, "test".into(), &progress).await;
        assert!(result.is_err());
        assert!(
            !default_data.exists(),
            "a location error must never create a fallback empty library"
        );
        drop(progress);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logging_rotates_previous_run_and_is_optional() {
        let root = std::env::temp_dir().join(format!("tk-startup-log-{}", uuid::Uuid::now_v7()));
        {
            let progress = StartupProgress::default();
            progress.start_logging(&root, "test-1");
            progress.stage("copy", "正在创建升级副本");
            progress.finish(None);
        }
        let old = fs::read(root.join("logs/startup-latest.jsonl")).unwrap();
        {
            let progress = StartupProgress::default();
            progress.start_logging(&root, "test-2");
            progress.finish(Some("synthetic error".into()));
            assert_eq!(
                fs::read(root.join("logs/startup-previous.jsonl")).unwrap(),
                old
            );
            assert!(
                fs::read_to_string(root.join("logs/startup-latest.jsonl"))
                    .unwrap()
                    .contains("synthetic error")
            );
        }
        let file = root.join("not-a-directory");
        fs::write(&file, b"fixture").unwrap();
        let progress = StartupProgress::default();
        progress.start_logging(&file, "test");
        progress.finish(None);
        assert!(progress.snapshot().log_path.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
