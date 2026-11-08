import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct TaskStateChip: View {
  let state: TaskState

  var body: some View {
    Text(state.title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(state == .in_progress ? MacTheme.onAccent : state.color)
      .padding(.horizontal, MacTheme.Space.x9).padding(.vertical, MacTheme.Space.x3)
      .background(
        state == .in_progress ? MacTheme.accent : state.color.opacity(0.14), in: Capsule())
  }
}

struct TaskPriorityChip: View {
  let priority: TaskPriority

  var body: some View {
    Text(priority.title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(MacTheme.secondary)
      .padding(.horizontal, MacTheme.Space.x8).padding(.vertical, MacTheme.Space.x2)
      .background(MacTheme.well, in: Capsule())
  }
}

struct TaskOwnerChip: View {
  let name: String?

  var body: some View {
    if let name {
      HStack(spacing: MacTheme.Space.x5) {
        Circle().fill(MacTheme.ink.opacity(0.75)).frame(
          width: DesignTokens.Layout.taskDot, height: DesignTokens.Layout.taskDot)
        Text(name).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold)).lineLimit(1)
      }
      .foregroundStyle(MacTheme.ink)
      .padding(.leading, MacTheme.Space.x6).padding(.trailing, MacTheme.Space.x8)
      .padding(.vertical, MacTheme.Space.x3).background(taskOwnerColor(name), in: Capsule())
    } else {
      Text("—").font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
    }
  }
}

@MainActor extension EngineClient {
  /// The name of the pane that owns `task`; its id while the pane list has no name for it.
  func ownerName(of task: TaskRecord) -> String? {
    guard let owner = task.ownerPaneId else { return nil }
    return panes.first { $0.paneId == owner }?.name ?? owner
  }

  func commentCount(of task: TaskRecord) -> Int {
    taskDetails[task.taskId]?.comments.count ?? 0
  }
}

@MainActor func taskOwnerColor(_ name: String) -> Color {
  switch TaskPresentation.ownerPlate(name) {
  case .rose: MacTheme.agentRose
  case .blue: MacTheme.agentBlue
  case .green: MacTheme.agentGreen
  case .sand: MacTheme.agentSand
  }
}

@MainActor extension MacTheme {
  static func color(for role: TaskRole) -> Color {
    switch role {
    case .accent: accent
    case .secondary: secondary
    case .warning: warning
    case .success: success
    case .danger: urgent
    }
  }
}

@MainActor extension TaskState {
  var title: String { TaskPresentation.title(self) }
  var color: Color { MacTheme.color(for: TaskPresentation.role(self)) }
}

@MainActor extension TaskPriority {
  static let all: [TaskPriority] = TaskPresentation.priorities
  var title: String { TaskPresentation.title(self) }
}

@MainActor extension TaskReviewBadge {
  var title: String { TaskPresentation.title(self) }
  var color: Color { MacTheme.color(for: TaskPresentation.role(self)) }
}

extension BlockingKind {
  var title: String { TaskPresentation.title(self) }
}

extension AttemptResult {
  var title: String { TaskPresentation.title(self) }
}
