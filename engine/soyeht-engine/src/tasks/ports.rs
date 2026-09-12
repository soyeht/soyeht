//! Persistence reads used by task rules.

use soyeht_protocol::{Attempt, Comment, LoopConfig, Subitem, Task, TaskMove, TaskState};
use soyeht_store::{
    database::{Result as StoreResult, Store},
    tasks::{CommentPage, DispatchCheckpoint, FeedbackState},
};

pub trait TaskStorePort {
    fn task(&self, workspace: &str, task: &str) -> StoreResult<Option<Task>>;
    fn list_tasks(
        &self,
        workspace: &str,
        state: Option<&TaskState>,
        limit: u32,
        offset: u32,
    ) -> StoreResult<Vec<Task>>;
    fn subitems(&self, workspace: &str, task: &str) -> StoreResult<Vec<Subitem>>;
    fn comments(
        &self,
        workspace: &str,
        task: &str,
        before: Option<u64>,
        limit: u32,
    ) -> StoreResult<Vec<Comment>>;
    fn comments_page(
        &self,
        workspace: &str,
        task: &str,
        before: Option<u64>,
        limit: u32,
    ) -> StoreResult<CommentPage>;
    fn attempts(&self, workspace: &str, task: &str) -> StoreResult<Vec<Attempt>>;
    fn moves(&self, workspace: &str, task: &str) -> StoreResult<Vec<TaskMove>>;
    fn dependencies(&self, workspace: &str, task: &str) -> StoreResult<Vec<String>>;
    fn dependency_path_exists(&self, workspace: &str, from: &str, to: &str) -> StoreResult<bool>;
    fn active_task_for_pane(&self, pane: &str) -> StoreResult<Option<Task>>;
    fn loop_config(&self, pane: &str) -> StoreResult<Option<(LoopConfig, u64)>>;
    fn feedback_state(&self, workspace: &str, task: &str) -> StoreResult<Option<FeedbackState>>;
    fn dispatch_checkpoint(
        &self,
        workspace: &str,
        task: &str,
    ) -> StoreResult<Option<DispatchCheckpoint>>;
}

impl TaskStorePort for Store {
    fn task(&self, workspace: &str, task: &str) -> StoreResult<Option<Task>> {
        Store::task(self, workspace, task)
    }

    fn list_tasks(
        &self,
        workspace: &str,
        state: Option<&TaskState>,
        limit: u32,
        offset: u32,
    ) -> StoreResult<Vec<Task>> {
        Store::list_tasks(self, workspace, state, limit, offset)
    }

    fn subitems(&self, workspace: &str, task: &str) -> StoreResult<Vec<Subitem>> {
        Store::task_subitems(self, workspace, task)
    }

    fn comments(
        &self,
        workspace: &str,
        task: &str,
        before: Option<u64>,
        limit: u32,
    ) -> StoreResult<Vec<Comment>> {
        Store::task_comments(self, workspace, task, before, limit)
    }

    fn comments_page(
        &self,
        workspace: &str,
        task: &str,
        before: Option<u64>,
        limit: u32,
    ) -> StoreResult<CommentPage> {
        Store::task_comments_page(self, workspace, task, before, limit)
    }

    fn attempts(&self, workspace: &str, task: &str) -> StoreResult<Vec<Attempt>> {
        Store::task_attempts(self, workspace, task)
    }

    fn moves(&self, workspace: &str, task: &str) -> StoreResult<Vec<TaskMove>> {
        Store::task_moves(self, workspace, task)
    }

    fn dependencies(&self, workspace: &str, task: &str) -> StoreResult<Vec<String>> {
        Store::task_dependencies(self, workspace, task)
    }

    fn dependency_path_exists(&self, workspace: &str, from: &str, to: &str) -> StoreResult<bool> {
        Store::dependency_path_exists(self, workspace, from, to)
    }

    fn active_task_for_pane(&self, pane: &str) -> StoreResult<Option<Task>> {
        Store::active_task_for_pane(self, pane)
    }

    fn loop_config(&self, pane: &str) -> StoreResult<Option<(LoopConfig, u64)>> {
        Store::loop_config(self, pane)
    }

    fn feedback_state(&self, workspace: &str, task: &str) -> StoreResult<Option<FeedbackState>> {
        Store::feedback_state(self, workspace, task)
    }

    fn dispatch_checkpoint(
        &self,
        workspace: &str,
        task: &str,
    ) -> StoreResult<Option<DispatchCheckpoint>> {
        Store::dispatch_checkpoint(self, workspace, task)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    pub struct MemoryTaskStore {
        pub tasks: BTreeMap<(String, String), Task>,
        pub edges: BTreeMap<(String, String), Vec<String>>,
    }

    impl TaskStorePort for MemoryTaskStore {
        fn task(&self, workspace: &str, task: &str) -> StoreResult<Option<Task>> {
            Ok(self.tasks.get(&(workspace.into(), task.into())).cloned())
        }

        fn list_tasks(
            &self,
            workspace: &str,
            state: Option<&TaskState>,
            limit: u32,
            offset: u32,
        ) -> StoreResult<Vec<Task>> {
            let mut items: Vec<_> = self
                .tasks
                .iter()
                .filter(|((owner, _), task)| {
                    owner == workspace && state.is_none_or(|state| &task.state == state)
                })
                .map(|(_, task)| task.clone())
                .collect();
            items.sort_by(|left, right| left.task_id.cmp(&right.task_id));
            Ok(items
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .collect())
        }

        fn subitems(&self, _: &str, _: &str) -> StoreResult<Vec<Subitem>> {
            Ok(Vec::new())
        }

        fn comments(&self, _: &str, _: &str, _: Option<u64>, _: u32) -> StoreResult<Vec<Comment>> {
            Ok(Vec::new())
        }

        fn comments_page(
            &self,
            _: &str,
            _: &str,
            _: Option<u64>,
            _: u32,
        ) -> StoreResult<CommentPage> {
            Ok(CommentPage {
                items: Vec::new(),
                next_before: None,
            })
        }

        fn attempts(&self, _: &str, _: &str) -> StoreResult<Vec<Attempt>> {
            Ok(Vec::new())
        }

        fn moves(&self, _: &str, _: &str) -> StoreResult<Vec<TaskMove>> {
            Ok(Vec::new())
        }

        fn dependencies(&self, workspace: &str, task: &str) -> StoreResult<Vec<String>> {
            Ok(self
                .edges
                .get(&(workspace.into(), task.into()))
                .cloned()
                .unwrap_or_default())
        }

        fn dependency_path_exists(
            &self,
            workspace: &str,
            from: &str,
            to: &str,
        ) -> StoreResult<bool> {
            let mut pending = vec![from.to_owned()];
            let mut visited = std::collections::BTreeSet::new();
            while let Some(node) = pending.pop() {
                if !visited.insert(node.clone()) {
                    continue;
                }
                let next = self
                    .edges
                    .get(&(workspace.into(), node))
                    .cloned()
                    .unwrap_or_default();
                if next.iter().any(|item| item == to) {
                    return Ok(true);
                }
                pending.extend(next);
            }
            Ok(false)
        }

        fn active_task_for_pane(&self, pane: &str) -> StoreResult<Option<Task>> {
            Ok(self
                .tasks
                .values()
                .find(|task| task.owner_pane_id.as_deref() == Some(pane))
                .cloned())
        }

        fn loop_config(&self, _: &str) -> StoreResult<Option<(LoopConfig, u64)>> {
            Ok(None)
        }

        fn feedback_state(&self, _: &str, _: &str) -> StoreResult<Option<FeedbackState>> {
            Ok(None)
        }

        fn dispatch_checkpoint(&self, _: &str, _: &str) -> StoreResult<Option<DispatchCheckpoint>> {
            Ok(None)
        }
    }
}
