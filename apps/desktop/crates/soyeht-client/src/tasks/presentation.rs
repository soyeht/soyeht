//! What the tasks say and how they sort.

use soyeht_protocol::{
    Attempt, AttemptResult, Blocking, BlockingKind, Comment, ContractChange, OutOfScopeEdit,
    Subitem, SubitemState, Task, TaskExecutor, TaskPriority, TaskReviewBadge, TaskShowResult,
    TaskState,
};

pub const NO_TASK_READY: &str = "No task is ready";
pub const EMPTY_TITLE: &str = "No tasks";
pub const EMPTY_DETAIL: &str =
    "Create a task for this workspace, then dispatch it or let an idle pane claim it.";

/// The color roles the task surface paints with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Accent,
    Secondary,
    Warning,
    Success,
    Danger,
}

/// The plates an owner's name is painted on, by their place in the palette's agent plates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plate {
    Rose,
    Blue,
    Green,
    Sand,
}

impl Plate {
    /// The index of the plate among the palette's agent plates (blue, green, rose, sand).
    pub fn index(self) -> usize {
        match self {
            Self::Blue => 0,
            Self::Green => 1,
            Self::Rose => 2,
            Self::Sand => 3,
        }
    }
}

pub fn state_title(state: &TaskState) -> &'static str {
    match state {
        TaskState::Todo => "To do",
        TaskState::InProgress => "In progress",
        TaskState::InReview => "In review",
        TaskState::Completed => "Completed",
        TaskState::Cancelled => "Cancelled",
    }
}

pub fn state_role(state: &TaskState) -> Role {
    match state {
        TaskState::Todo => Role::Secondary,
        TaskState::InProgress => Role::Accent,
        TaskState::InReview => Role::Warning,
        TaskState::Completed => Role::Success,
        TaskState::Cancelled => Role::Danger,
    }
}

/// The board's columns: cancelled tasks get a column only when there are some.
pub fn board_states(tasks: &[&Task]) -> Vec<TaskState> {
    with_cancelled(
        &[
            TaskState::Todo,
            TaskState::InProgress,
            TaskState::InReview,
            TaskState::Completed,
        ],
        tasks,
    )
}

/// The list's groups, the work in hand first.
pub fn list_states(tasks: &[&Task]) -> Vec<TaskState> {
    with_cancelled(
        &[
            TaskState::InProgress,
            TaskState::InReview,
            TaskState::Todo,
            TaskState::Completed,
        ],
        tasks,
    )
}

fn with_cancelled(states: &[TaskState], tasks: &[&Task]) -> Vec<TaskState> {
    let mut states = states.to_vec();
    if tasks.iter().any(|task| task.state == TaskState::Cancelled) {
        states.push(TaskState::Cancelled);
    }
    states
}

pub const PRIORITIES: [TaskPriority; 4] = [
    TaskPriority::Urgent,
    TaskPriority::High,
    TaskPriority::Medium,
    TaskPriority::Low,
];

pub fn priority_title(priority: &TaskPriority) -> &'static str {
    match priority {
        TaskPriority::Urgent => "Urgent",
        TaskPriority::High => "High",
        TaskPriority::Medium => "Medium",
        TaskPriority::Low => "Low",
    }
}

/// Within a list group: the most urgent first, then by key.
pub fn list_order(left: &Task, right: &Task) -> std::cmp::Ordering {
    let rank = |task: &Task| {
        PRIORITIES
            .iter()
            .position(|priority| *priority == task.priority)
            .unwrap_or(PRIORITIES.len())
    };
    rank(left)
        .cmp(&rank(right))
        .then_with(|| left.task_id.cmp(&right.task_id))
}

pub fn badge_title(badge: &TaskReviewBadge) -> &'static str {
    match badge {
        TaskReviewBadge::YourTurn => "YOUR TURN",
        TaskReviewBadge::SystemTurn => "SYSTEM'S TURN",
    }
}

pub fn badge_role(badge: &TaskReviewBadge) -> Role {
    match badge {
        TaskReviewBadge::YourTurn => Role::Accent,
        TaskReviewBadge::SystemTurn => Role::Warning,
    }
}

pub fn blocking_title(kind: &BlockingKind) -> &'static str {
    match kind {
        BlockingKind::Dependency => "dependency",
        BlockingKind::MissingInformation => "missing information",
        BlockingKind::ExternalError => "external error",
    }
}

pub fn attempt_result_title(result: &AttemptResult) -> &'static str {
    match result {
        AttemptResult::Completed => "Completed",
        AttemptResult::Returned => "Returned",
        AttemptResult::Failed => "Failed",
        AttemptResult::Cancelled => "Cancelled",
    }
}

/// The plate an owner's name is painted on: the same name always gets the same one.
pub fn owner_plate(name: &str) -> Plate {
    let plates = [Plate::Rose, Plate::Blue, Plate::Green, Plate::Sand];
    let slot = name.bytes().fold(0usize, |total, byte| {
        (total + usize::from(byte)) % plates.len()
    });
    plates[slot]
}

/// "YOUR TURN · available to claim": the review badge, when there is one, ahead of the phrase.
pub fn status_line(task: &Task) -> String {
    match &task.review_badge {
        Some(badge) => format!("{} \u{b7} {}", badge_title(badge), task.phrase),
        None => task.phrase.clone(),
    }
}

/// The dot before the status line: the review badge's color, else a warning while blocked, else
/// the state's.
pub fn indicator(task: &Task) -> Role {
    if let Some(badge) = &task.review_badge {
        return badge_role(badge);
    }
    if task.blocking.is_some() {
        return Role::Warning;
    }
    state_role(&task.state)
}

/// "10 tasks · 3 in progress · 2 need you".
pub fn summary(tasks: &[&Task], attention: usize) -> String {
    let active = tasks
        .iter()
        .filter(|task| task.state == TaskState::InProgress)
        .count();
    format!(
        "{} tasks \u{b7} {active} in progress \u{b7} {attention} need you",
        tasks.len()
    )
}

/// The tasks the Dispatch menu offers: to do, and nobody owns them.
pub fn ready_to_dispatch<'a>(tasks: &[&'a Task]) -> Vec<&'a Task> {
    tasks
        .iter()
        .copied()
        .filter(|task| task.state == TaskState::Todo && task.owner_pane_id.is_none())
        .collect()
}

pub fn dispatch_label(task: &Task) -> String {
    format!("{} \u{b7} {}", task.task_id, task.title)
}

/// Who runs a task: the workspace's default, or the agent, model and effort it names.
pub fn runner_name(executor: Option<&TaskExecutor>) -> String {
    match executor {
        Some(executor) if !executor.workspace_default => [
            executor.harness.as_deref(),
            executor.model.as_deref(),
            executor.effort.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{b7} "),
        _ => "Workspace default".to_owned(),
    }
}

pub fn subitems_title(subitems: &[Subitem]) -> String {
    let done = subitems
        .iter()
        .filter(|item| item.state == SubitemState::Done)
        .count();
    format!("SUBITEMS \u{b7} {done} OF {}", subitems.len())
}

pub fn attempt_line(attempt: &Attempt) -> String {
    format!(
        "Attempt {} \u{b7} {} \u{b7} {}",
        attempt.n, attempt.harness, attempt.model
    )
}

pub fn attempt_outcome(attempt: &Attempt) -> &'static str {
    attempt
        .result
        .as_ref()
        .map_or("active", attempt_result_title)
}

pub fn blocking_heading(blocking: &Blocking) -> String {
    format!(
        "BLOCKING \u{b7} {}",
        blocking_title(&blocking.kind).to_uppercase()
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub id: String,
    /// Who wrote it; none for a move between states, which the system records.
    pub author: Option<String>,
    pub text: String,
    pub at: String,
}

/// The moves between states and the comments, oldest first.
pub fn timeline(detail: &TaskShowResult) -> Vec<TimelineEntry> {
    let moves = detail
        .history
        .iter()
        .enumerate()
        .map(|(index, step)| TimelineEntry {
            id: format!("move-{index}-{}", step.at),
            author: None,
            text: format!(
                "{} \u{b7} {} \u{2192} {} \u{b7} {}",
                step.actor,
                state_title(&step.from),
                state_title(&step.to),
                step.reason
            ),
            at: step.at.clone(),
        });
    let comments = detail.comments.iter().map(|comment| TimelineEntry {
        id: format!("comment-{}", comment.comment_id),
        author: Some(comment.author.clone()),
        text: comment.text.clone(),
        at: comment.at.clone(),
    });
    let mut entries: Vec<TimelineEntry> = moves.chain(comments).collect();
    entries.sort_by(|left, right| left.at.cmp(&right.at));
    entries
}

/// The lines of the comments that name a change ("a → b"), the reviewer's findings.
pub fn drift_findings(comments: &[Comment]) -> Vec<String> {
    comments
        .iter()
        .flat_map(|comment| comment.text.lines())
        .filter(|line| line.contains('\u{2192}'))
        .map(str::to_owned)
        .collect()
}

pub fn contract_change(change: &ContractChange) -> String {
    let label = serde_json::to_value(&change.label).map_or_else(
        |_| String::new(),
        |value| value.as_str().unwrap_or_default().to_owned(),
    );
    format!(
        "{}::{} \u{b7} {label} \u{b7} {}",
        change.path, change.name, change.contract_ref
    )
}

pub fn out_of_scope_edit(edit: &OutOfScopeEdit) -> String {
    format!("{} \u{b7} {}", edit.path, edit.reason)
}

/// The fields of a new task that are still empty, by the names the form gives them.
pub fn missing_required_fields(
    title: &str,
    description: &str,
    must_not_change: &str,
) -> Vec<&'static str> {
    [
        ("title", title),
        ("description", description),
        ("MUST NOT CHANGE", must_not_change),
    ]
    .into_iter()
    .filter(|(_, value)| value.trim().is_empty())
    .map(|(name, _)| name)
    .collect()
}

/// What the new-task form says under its fields: what is missing, or how to create.
pub fn action_hint(
    missing: &[&str],
    workspace_default: bool,
    harness: &str,
    shortcuts: &str,
) -> String {
    if !missing.is_empty() {
        return format!("Required: {}", missing.join(", "));
    }
    if !workspace_default && harness.is_empty() {
        return "Required: choose an agent".to_owned();
    }
    shortcuts.to_owned()
}

/// The dependencies pill: "dependencies", "after WS-1", "after WS-1 +2".
pub fn dependency_title(dependencies: &[String]) -> String {
    let mut sorted: Vec<&String> = dependencies.iter().collect();
    sorted.sort();
    sorted.dedup();
    match sorted.as_slice() {
        [] => "dependencies".to_owned(),
        [only] => format!("after {only}"),
        [first, rest @ ..] => format!("after {first} +{}", rest.len()),
    }
}

/// The runner pill: the workspace default, or the agent with the model and effort chosen.
pub fn runner_title(
    workspace_default: bool,
    harness_name: Option<&str>,
    model_name: Option<&str>,
    effort_name: Option<&str>,
) -> String {
    if workspace_default {
        return "Workspace default".to_owned();
    }
    [
        Some(harness_name.unwrap_or("Choose an agent")),
        model_name,
        effort_name,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{b7} ")
}
