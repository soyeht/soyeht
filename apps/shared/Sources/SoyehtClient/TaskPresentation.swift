import Foundation
import SoyehtProtocol

/// The color roles the task surface paints with. Each client maps a role to its palette.
public enum TaskRole: String, Sendable {
  case accent, secondary, warning, success, danger
}

/// The plates an owner's name is painted on (the palette's agent plates).
public enum TaskOwnerPlate: String, Sendable {
  case rose, blue, green, sand
}

/// What the workspace's task board, list and detail say and how they sort, shared with the
/// desktop client through `contracts/vectors/tasks.json`.
public enum TaskPresentation {
  public static let noTaskReady = "No task is ready"
  public static let emptyTitle = "No tasks"
  public static let emptyDetail =
    "Create a task for this workspace, then dispatch it or let an idle pane claim it."

  public static func title(_ state: TaskState) -> String {
    switch state {
    case .todo: "To do"
    case .in_progress: "In progress"
    case .in_review: "In review"
    case .completed: "Completed"
    case .cancelled: "Cancelled"
    }
  }

  public static func role(_ state: TaskState) -> TaskRole {
    switch state {
    case .todo: .secondary
    case .in_progress: .accent
    case .in_review: .warning
    case .completed: .success
    case .cancelled: .danger
    }
  }

  /// The board's columns: cancelled tasks get a column only when there are some.
  public static func boardStates(_ tasks: [TaskRecord]) -> [TaskState] {
    withCancelled([.todo, .in_progress, .in_review, .completed], tasks)
  }

  /// The list's groups, the work in hand first.
  public static func listStates(_ tasks: [TaskRecord]) -> [TaskState] {
    withCancelled([.in_progress, .in_review, .todo, .completed], tasks)
  }

  private static func withCancelled(_ states: [TaskState], _ tasks: [TaskRecord]) -> [TaskState] {
    tasks.contains { $0.state == .cancelled } ? states + [.cancelled] : states
  }

  public static let priorities: [TaskPriority] = [.urgent, .high, .medium, .low]

  public static func title(_ priority: TaskPriority) -> String {
    priority.rawValue.replacingOccurrences(of: "_", with: " ").capitalized
  }

  /// Within a list group: the most urgent first, then by key.
  public static func listPrecedes(_ left: TaskRecord, _ right: TaskRecord) -> Bool {
    let leftIndex = priorities.firstIndex(of: left.priority) ?? priorities.count
    let rightIndex = priorities.firstIndex(of: right.priority) ?? priorities.count
    if leftIndex != rightIndex { return leftIndex < rightIndex }
    return left.taskId < right.taskId
  }

  public static func title(_ badge: TaskReviewBadge) -> String {
    badge == .your_turn ? "YOUR TURN" : "SYSTEM'S TURN"
  }

  public static func role(_ badge: TaskReviewBadge) -> TaskRole {
    badge == .your_turn ? .accent : .warning
  }

  public static func title(_ kind: BlockingKind) -> String {
    kind.rawValue.replacingOccurrences(of: "_", with: " ")
  }

  public static func title(_ result: AttemptResult) -> String { result.rawValue.capitalized }

  /// The plate an owner's name is painted on: the same name always gets the same one.
  public static func ownerPlate(_ name: String) -> TaskOwnerPlate {
    let plates: [TaskOwnerPlate] = [.rose, .blue, .green, .sand]
    return plates[name.utf8.reduce(0) { ($0 + Int($1)) % plates.count }]
  }

  /// "YOUR TURN · available to claim": the review badge, when there is one, ahead of the phrase.
  public static func statusLine(_ task: TaskRecord) -> String {
    task.reviewBadge.map { "\(title($0)) · \(task.phrase)" } ?? task.phrase
  }

  /// The dot before the status line: the review badge's color, else a warning while blocked,
  /// else the state's.
  public static func indicator(_ task: TaskRecord) -> TaskRole {
    if let badge = task.reviewBadge { return role(badge) }
    if task.blocking != nil { return .warning }
    return role(task.state)
  }

  /// "10 tasks · 3 in progress · 2 need you".
  public static func summary(tasks: [TaskRecord], attention: Int) -> String {
    let active = tasks.filter { $0.state == .in_progress }.count
    return "\(tasks.count) tasks · \(active) in progress · \(attention) need you"
  }

  /// The tasks the Dispatch menu offers: to do, and nobody owns them.
  public static func readyToDispatch(_ tasks: [TaskRecord]) -> [TaskRecord] {
    tasks.filter { $0.state == .todo && $0.ownerPaneId == nil }
  }

  public static func dispatchLabel(_ task: TaskRecord) -> String {
    "\(task.taskId) · \(task.title)"
  }

  /// Who runs a task: the workspace's default, or the agent, model and effort it names.
  public static func runnerName(_ task: TaskRecord) -> String {
    guard let executor = task.executor, !executor.workspaceDefault else {
      return "Workspace default"
    }
    return [executor.harness, executor.model, executor.effort]
      .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
  }

  public static func subitemsTitle(_ subitems: [Subitem]) -> String {
    "SUBITEMS · \(subitems.filter { $0.state == .done }.count) OF \(subitems.count)"
  }

  public static func attemptLine(_ attempt: Attempt) -> String {
    "Attempt \(attempt.n) · \(attempt.harness) · \(attempt.model)"
  }

  public static func attemptOutcome(_ attempt: Attempt) -> String {
    attempt.result.map(title) ?? "active"
  }

  public struct TimelineEntry: Equatable, Sendable {
    public let id: String
    /// Who wrote it; none for a move between states, which the system records.
    public let author: String?
    public let text: String
    public let at: String
  }

  /// The moves between states and the comments, oldest first.
  public static func timeline(_ detail: TaskShowResult) -> [TimelineEntry] {
    let moves = detail.history.enumerated().map { index, move in
      TimelineEntry(
        id: "move-\(index)-\(move.at)", author: nil,
        text: "\(move.actor) · \(title(move.from)) → \(title(move.to)) · \(move.reason)",
        at: move.at)
    }
    let comments = detail.comments.map { comment in
      TimelineEntry(
        id: "comment-\(comment.commentId)", author: comment.author, text: comment.text,
        at: comment.at)
    }
    return (moves + comments).sorted { $0.at < $1.at }
  }

  /// The lines of the comments that name a change ("a → b"), the reviewer's findings.
  public static func driftFindings(_ comments: [Comment]) -> [String] {
    comments.flatMap { comment in
      comment.text.split(separator: "\n").map(String.init).filter { $0.contains("→") }
    }
  }

  public static func contractChange(_ change: ContractChange) -> String {
    "\(change.path)::\(change.name) · \(change.label.rawValue) · \(change.contractRef)"
  }

  public static func outOfScopeEdit(_ edit: OutOfScopeEdit) -> String {
    "\(edit.path) · \(edit.reason)"
  }

  /// The fields of a new task that are still empty, by the names the form gives them.
  public static func missingRequiredFields(
    title: String, description: String, mustNotChange: String
  ) -> [String] {
    [("title", title), ("description", description), ("MUST NOT CHANGE", mustNotChange)]
      .compactMap { name, value in
        value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? name : nil
      }
  }

  /// What the new-task form says under its fields: what is missing, or how to create.
  public static func actionHint(
    missing: [String], workspaceDefault: Bool, harness: String, shortcuts: String
  ) -> String {
    if !missing.isEmpty { return "Required: \(missing.joined(separator: ", "))" }
    if !workspaceDefault && harness.isEmpty { return "Required: choose an agent" }
    return shortcuts
  }

  /// The dependencies pill: "dependencies", "after WS-1", "after WS-1 +2".
  public static func dependencyTitle(_ dependencies: Set<String>) -> String {
    guard let first = dependencies.sorted().first else { return "dependencies" }
    return dependencies.count == 1 ? "after \(first)" : "after \(first) +\(dependencies.count - 1)"
  }

  /// The runner pill: the workspace default, or the agent with the model and effort chosen.
  public static func runnerTitle(
    workspaceDefault: Bool, harnessName: String?, modelName: String?, effortName: String?
  ) -> String {
    guard !workspaceDefault else { return "Workspace default" }
    return [harnessName ?? "Choose an agent", modelName, effortName]
      .compactMap { $0 }.joined(separator: " · ")
  }
}
