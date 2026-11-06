use super::models::{CommandError, CommandResult};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: String,
    pub completed: usize,
    pub total: Option<usize>,
    pub cancellable: bool,
    pub cancelled: bool,
    pub finished: bool,
    #[serde(skip)]
    started: bool,
}
type Shared = Arc<Mutex<Progress>>;
static TASKS: LazyLock<Mutex<VecDeque<(String, Shared)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
fn entry(id: &str) -> CommandResult<Shared> {
    let id = uuid::Uuid::parse_str(id)
        .map_err(|_| CommandError::validation("任务标识无效"))?
        .to_string();
    let mut entries = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, state)) = entries.iter().find(|(key, _)| key == &id) {
        return Ok(state.clone());
    }
    if entries.len() >= 128 {
        let removable = entries
            .iter()
            .position(|(_, state)| {
                let state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.finished || !state.started
            })
            .ok_or_else(|| CommandError::validation("后台任务过多，请等待完成"))?;
        entries.remove(removable);
    }
    let state = Arc::new(Mutex::new(Progress {
        phase: "等待处理".into(),
        completed: 0,
        total: None,
        cancellable: true,
        cancelled: false,
        finished: false,
        started: false,
    }));
    entries.push_back((id, state.clone()));
    Ok(state)
}
pub fn progress(id: &str) -> CommandResult<Progress> {
    Ok(entry(id)?.lock().unwrap_or_else(|e| e.into_inner()).clone())
}
pub fn cancel(id: &str) -> CommandResult<()> {
    let shared = entry(id)?;
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    if state.cancellable && !state.finished {
        state.cancelled = true;
    }
    Ok(())
}
#[derive(Clone, Default)]
pub struct Context(Option<Shared>);
impl Context {
    pub fn checkpoint(
        &self,
        phase: &str,
        completed: usize,
        total: Option<usize>,
    ) -> CommandResult<()> {
        if let Some(shared) = &self.0 {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            if state.cancelled {
                return Err(CommandError::new(
                    "TASK_CANCELLED",
                    "已取消，未创建导入草稿。",
                ));
            }
            state.phase = phase.into();
            state.completed = completed;
            state.total = total;
        }
        Ok(())
    }
    pub fn begin_commit(&self) -> CommandResult<()> {
        if let Some(shared) = &self.0 {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            if state.cancelled {
                return Err(CommandError::new(
                    "TASK_CANCELLED",
                    "已取消，未创建导入草稿。",
                ));
            }
            state.cancellable = false;
            state.phase = "正在保存导入草稿，请等待完成".into();
            state.total = None;
        }
        Ok(())
    }
}
pub struct Task(Context);
impl Task {
    pub fn start(id: Option<String>) -> CommandResult<Self> {
        let Some(id) = id else {
            return Ok(Self(Context::default()));
        };
        let shared = entry(&id)?;
        {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            if state.started {
                return Err(CommandError::validation("任务标识已使用，请重新开始"));
            }
            state.started = true;
        }
        let task = Self(Context(Some(shared)));
        task.0.checkpoint("等待读取文件", 0, None)?;
        Ok(task)
    }
    pub fn context(&self) -> Context {
        self.0.clone()
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        if let Some(shared) = &self.0.0 {
            let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
            state.finished = true;
            state.cancellable = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn formula_notice_uses_the_same_checked_word_converter() {
        let results = super::super::commands::check_formula_compatibility(vec![
            "x^2".into(),
            r"\genfrac{}{}{0pt}{}{5}{2}".into(),
            r"\begin{matrix}1&2\\3&4\end{matrix}".into(),
            r"\mathop{\rightarrow}\limits^{A}_{B}".into(),
        ])
        .await
        .unwrap();
        assert!(results[..3].iter().all(Option::is_none));
        assert!(results[3].as_ref().unwrap().contains("limits"));
        assert!(
            super::super::commands::check_formula_compatibility(vec!["x".into(); 65])
                .await
                .is_err()
        );
    }
    #[test]
    fn cancellation_before_start_and_at_checkpoints_is_observed() {
        let id = uuid::Uuid::now_v7().to_string();
        cancel(&id).unwrap();
        assert_eq!(
            Task::start(Some(id.clone())).err().unwrap().code,
            "TASK_CANCELLED"
        );
        assert!(progress(&id).unwrap().finished);
        let id = uuid::Uuid::now_v7().to_string();
        let task = Task::start(Some(id.clone())).unwrap();
        task.context().checkpoint("rows", 4, Some(10)).unwrap();
        assert_eq!(progress(&id).unwrap().completed, 4);
        cancel(&id).unwrap();
        assert_eq!(
            task.context().begin_commit().unwrap_err().code,
            "TASK_CANCELLED"
        );
    }
    #[test]
    fn cancellation_does_not_interrupt_a_committing_transaction() {
        let id = uuid::Uuid::now_v7().to_string();
        let task = Task::start(Some(id.clone())).unwrap();
        task.context().begin_commit().unwrap();
        cancel(&id).unwrap();
        assert!(!progress(&id).unwrap().cancelled);
        drop(task);
        assert!(progress(&id).unwrap().finished);
    }

    #[test]
    fn cancelled_recognition_stops_before_file_access_or_draft_creation() {
        let id = uuid::Uuid::now_v7().to_string();
        let task = Task::start(Some(id.clone())).unwrap();
        cancel(&id).unwrap();
        let word = super::super::docx::prepare_word_import_with_task(
            super::super::models::AnalyzeDocxRequestApi {
                input_path: "never-open-this.docx".into(),
            },
            &task.context(),
        );
        assert_eq!(word.err().unwrap().code, "TASK_CANCELLED");
        let excel = super::super::excel_import::prepare_import_with_task(
            super::super::models::ExcelImportRequestApi {
                input_path: "never-open-this.xlsx".into(),
                sheet_name: None,
            },
            &task.context(),
        );
        assert_eq!(excel.err().unwrap().code, "TASK_CANCELLED");
        assert!(task.context().begin_commit().is_err());
    }
}
