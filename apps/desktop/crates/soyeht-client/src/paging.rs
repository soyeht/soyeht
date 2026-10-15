//! Lists the engine hands over in pages, read to the end.

use crate::tasks::TaskUpdate;
use crate::{Event, Result, rpc::Rpc};
use soyeht_protocol::{
    PaneQueueListParams, PaneQueueListResult, Request, TaskListParams, TaskListResult,
    client_policy,
};

/// More than this many waiting messages are not read, as on the Mac client.
const QUEUE_LIMIT: usize = 1_000;

/// Every message waiting for an agent pane, and whether its queue is paused.
pub(crate) fn queue(rpc: &Rpc, pane_id: &str) -> Result<Event> {
    let (mut items, mut cursor) = (Vec::new(), None);
    loop {
        let page: PaneQueueListResult =
            serde_json::from_value(rpc.call(&Request::PaneQueueList(PaneQueueListParams {
                pane_id: pane_id.to_owned(),
                cursor: cursor.take(),
                limit: Some(client_policy::PAGE_SIZE_QUEUE),
            }))?)?;
        items.extend(page.items);
        cursor = page.next_cursor;
        if cursor.is_none() || items.len() >= QUEUE_LIMIT {
            return Ok(Event::Queue {
                pane_id: pane_id.to_owned(),
                items,
                paused: page.paused.unwrap_or(false),
            });
        }
    }
}

/// Every task of a workspace.
pub(crate) fn tasks(rpc: &Rpc, workspace_id: &str) -> Result<Event> {
    let (mut tasks, mut cursor) = (Vec::new(), None);
    loop {
        let page: TaskListResult =
            serde_json::from_value(rpc.call(&Request::TaskList(TaskListParams {
                workspace_id: workspace_id.to_owned(),
                state: None,
                limit: client_policy::PAGE_SIZE_TASKS,
                cursor: cursor.take(),
            }))?)?;
        tasks.extend(page.items);
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(Event::Task(TaskUpdate::Workspace {
                workspace_id: workspace_id.to_owned(),
                tasks,
            }));
        }
    }
}
