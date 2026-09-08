use std::{ops::Deref, path::PathBuf, sync::Arc};

use tokio::sync::{Mutex, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

use crate::db::Database;
use crate::licensing::LicenseService;

use super::models::{CommandError, CommandResult};

#[derive(Clone)]
pub struct AppState {
    database: Arc<RwLock<Option<Database>>>,
    maintenance: Arc<RwLock<()>>,
    scheduled_maintenance: Arc<Mutex<Option<OwnedRwLockWriteGuard<()>>>>,
    initialization_error: Arc<RwLock<Option<String>>>,
    retry_guard: Arc<Mutex<()>>,
    data_root: PathBuf,
    app_version: String,
    license_service: LicenseService,
}

/// Shared access to the current database for the lifetime of a command.
///
/// Keeping the owned read guard beside the cloned database prevents an
/// exclusive maintenance operation from starting until this lease is dropped.
pub struct DatabaseLease {
    database: Database,
    _maintenance_guard: OwnedRwLockReadGuard<()>,
}

impl Deref for DatabaseLease {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}

/// Exclusive access to the current database and all managed data files.
pub struct ExclusiveDatabaseLease {
    database: Database,
    _maintenance_guard: Option<OwnedRwLockWriteGuard<()>>,
}

impl Deref for ExclusiveDatabaseLease {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.database
    }
}

/// Shared maintenance lease for commands that may read managed files without
/// querying SQLite, such as exporting from a managed Word template.
pub struct MaintenanceReadLease {
    _maintenance_guard: OwnedRwLockReadGuard<()>,
}

/// Exclusive maintenance access that does not require a healthy database.
/// This is used to serialize cancellation with restore scheduling.
pub struct ExclusiveMaintenanceLease {
    _maintenance_guard: OwnedRwLockWriteGuard<()>,
}

/// Exclusive restore access, optionally carrying the healthy database. A
/// missing database enables the explicit rescue path while still freezing all
/// managed files until restart.
pub struct RestoreMaintenanceLease {
    database: Option<Database>,
    maintenance_guard: Option<OwnedRwLockWriteGuard<()>>,
}

impl RestoreMaintenanceLease {
    pub fn database(&self) -> Option<&Database> {
        self.database.as_ref()
    }
}

impl AppState {
    pub fn unavailable_with_license_roots(
        data_root: PathBuf,
        security_root: PathBuf,
        license_root: PathBuf,
        legacy_security_root: Option<PathBuf>,
        app_version: String,
        message: String,
    ) -> Self {
        let license_service = LicenseService::initialize_with_roots(
            security_root.clone(),
            license_root.clone(),
            legacy_security_root,
            app_version.clone(),
        )
        .unwrap_or_else(|error| {
            LicenseService::fallback_with_roots(
                security_root,
                license_root,
                app_version.clone(),
                error.message,
            )
        });
        Self {
            database: Arc::new(RwLock::new(None)),
            maintenance: Arc::new(RwLock::new(())),
            scheduled_maintenance: Arc::new(Mutex::new(None)),
            initialization_error: Arc::new(RwLock::new(Some(message))),
            retry_guard: Arc::new(Mutex::new(())),
            data_root,
            app_version,
            license_service,
        }
    }

    #[cfg(test)]
    pub async fn initialize(data_root: PathBuf, app_version: String) -> Self {
        let license_root = data_root.join(".licensing-test");
        Self::initialize_with_license_roots(
            data_root,
            license_root.clone(),
            license_root,
            None,
            app_version,
        )
        .await
    }

    pub async fn initialize_with_license_roots(
        data_root: PathBuf,
        security_root: PathBuf,
        license_root: PathBuf,
        legacy_security_root: Option<PathBuf>,
        app_version: String,
    ) -> Self {
        let license_service = LicenseService::initialize_with_roots(
            security_root.clone(),
            license_root.clone(),
            legacy_security_root,
            app_version.clone(),
        )
        .unwrap_or_else(|error| {
            LicenseService::fallback_with_roots(
                security_root,
                license_root,
                app_version.clone(),
                error.message,
            )
        });
        let database = Database::open(data_root.clone(), &app_version).await;
        match database {
            Ok(database) => Self {
                database: Arc::new(RwLock::new(Some(database))),
                maintenance: Arc::new(RwLock::new(())),
                scheduled_maintenance: Arc::new(Mutex::new(None)),
                initialization_error: Arc::new(RwLock::new(None)),
                retry_guard: Arc::new(Mutex::new(())),
                data_root,
                app_version,
                license_service,
            },
            Err(error) => Self {
                database: Arc::new(RwLock::new(None)),
                maintenance: Arc::new(RwLock::new(())),
                scheduled_maintenance: Arc::new(Mutex::new(None)),
                initialization_error: Arc::new(RwLock::new(Some(error.to_string()))),
                retry_guard: Arc::new(Mutex::new(())),
                data_root,
                app_version,
                license_service,
            },
        }
    }

    pub async fn database(&self) -> CommandResult<DatabaseLease> {
        let maintenance_guard = Arc::clone(&self.maintenance).read_owned().await;
        let database = self.current_database().await?;
        Ok(DatabaseLease {
            database,
            _maintenance_guard: maintenance_guard,
        })
    }

    pub async fn exclusive_database(&self) -> CommandResult<ExclusiveDatabaseLease> {
        let maintenance_guard = Arc::clone(&self.maintenance).write_owned().await;
        let database = self.current_database().await?;
        Ok(ExclusiveDatabaseLease {
            database,
            _maintenance_guard: Some(maintenance_guard),
        })
    }

    pub async fn exclusive_maintenance_lease(&self) -> ExclusiveMaintenanceLease {
        ExclusiveMaintenanceLease {
            _maintenance_guard: Arc::clone(&self.maintenance).write_owned().await,
        }
    }

    pub async fn restore_maintenance_lease(&self) -> RestoreMaintenanceLease {
        let maintenance_guard = Arc::clone(&self.maintenance).write_owned().await;
        let database = self.database.read().await.clone();
        RestoreMaintenanceLease {
            database,
            maintenance_guard: Some(maintenance_guard),
        }
    }

    /// Keep the exclusive lock alive after a maintenance switch is committed.
    /// New commands remain blocked during the short interval before restart,
    /// so restore and data-move snapshots exactly match the retained old data.
    pub async fn hold_restore_maintenance_until_restart(&self, mut lease: RestoreMaintenanceLease) {
        let guard = lease
            .maintenance_guard
            .take()
            .expect("a fresh restore maintenance lease must own its write guard");
        let mut scheduled = self.scheduled_maintenance.lock().await;
        debug_assert!(scheduled.is_none());
        *scheduled = Some(guard);
    }

    pub async fn maintenance_read_lease(&self) -> MaintenanceReadLease {
        MaintenanceReadLease {
            _maintenance_guard: Arc::clone(&self.maintenance).read_owned().await,
        }
    }

    async fn current_database(&self) -> CommandResult<Database> {
        self.database.read().await.clone().ok_or_else(|| {
            CommandError::new(
                "DATABASE_UNAVAILABLE",
                "本地数据库当前不可用，写入操作已停止。",
            )
        })
    }

    pub async fn initialization_error(&self) -> Option<String> {
        self.initialization_error.read().await.clone()
    }

    pub async fn retry_initialize(&self) -> CommandResult<()> {
        let _retry_guard = self.retry_guard.lock().await;
        let _maintenance_guard = Arc::clone(&self.maintenance).write_owned().await;
        if self.database.read().await.is_some() {
            return Ok(());
        }
        if let Err(error) = crate::restore::apply_pending_restore(&self.data_root).await {
            let message = format!("恢复维护仍未完成：{}", error.message);
            *self.initialization_error.write().await = Some(message.clone());
            return Err(CommandError::new("RESTORE_MAINTENANCE_UNRESOLVED", message));
        }
        match Database::open(self.data_root.clone(), &self.app_version).await {
            Ok(database) => {
                let previous = self.database.write().await.replace(database);
                *self.initialization_error.write().await = None;
                if let Some(previous) = previous {
                    previous.close().await;
                }
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                *self.initialization_error.write().await = Some(message.clone());
                Err(CommandError::new(
                    "DATABASE_INITIALIZATION_FAILED",
                    format!("本地数据库重新加载失败：{message}"),
                ))
            }
        }
    }

    pub fn data_root(&self) -> &PathBuf {
        &self.data_root
    }
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    pub fn license(&self) -> &LicenseService {
        &self.license_service
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tokio::{sync::oneshot, task::yield_now};
    use uuid::Uuid;

    use super::AppState;

    async fn test_state() -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-maintenance-lock-test-{}",
            Uuid::now_v7().simple()
        ));
        let state = AppState::initialize(root.clone(), "test".to_owned()).await;
        assert!(
            state.initialization_error().await.is_none(),
            "test database should initialize"
        );
        (state, root)
    }

    async fn close_and_remove(state: AppState, root: PathBuf) {
        if let Some(database) = state.database.write().await.take() {
            database.close().await;
        }
        drop(state);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn exclusive_database_waits_for_existing_read_lease() {
        let (state, root) = test_state().await;
        let read_lease = state.database().await.expect("read lease");
        let contender = state.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (acquired_tx, mut acquired_rx) = oneshot::channel();

        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            let lease = contender.exclusive_database().await;
            let _ = acquired_tx.send(());
            lease
        });

        started_rx.await.expect("exclusive task started");
        yield_now().await;
        assert!(
            matches!(
                acquired_rx.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ),
            "exclusive lease must wait while a read lease is alive"
        );

        drop(read_lease);
        acquired_rx.await.expect("exclusive lease acquired");
        let exclusive_lease = task.await.expect("exclusive task joined").expect("lease");
        drop(exclusive_lease);
        close_and_remove(state, root).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn new_database_read_waits_during_exclusive_lease() {
        let (state, root) = test_state().await;
        let exclusive_lease = state.exclusive_database().await.expect("exclusive lease");
        let contender = state.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (acquired_tx, mut acquired_rx) = oneshot::channel();

        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            let lease = contender.database().await;
            let _ = acquired_tx.send(());
            lease
        });

        started_rx.await.expect("read task started");
        yield_now().await;
        assert!(
            matches!(
                acquired_rx.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ),
            "new read leases must wait while maintenance is exclusive"
        );

        drop(exclusive_lease);
        acquired_rx.await.expect("read lease acquired");
        let read_lease = task.await.expect("read task joined").expect("lease");
        drop(read_lease);
        close_and_remove(state, root).await;
    }
}
