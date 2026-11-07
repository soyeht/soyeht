import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct TaskBoard: View {
  @Bindable var client: EngineClient
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  private var visibleStates: [TaskState] { TaskPresentation.boardStates(tasks) }

  var body: some View {
    ScrollView([.horizontal, .vertical]) {
      HStack(alignment: .top, spacing: MacTheme.Space.x16) {
        ForEach(visibleStates, id: \.rawValue) { state in
          TaskColumn(
            client: client, state: state, tasks: tasks.filter { $0.state == state }, open: open)
        }
      }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }.scrollIndicators(.never)
  }
}

struct TaskColumn: View {
  @Bindable var client: EngineClient
  let state: TaskState
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack(spacing: MacTheme.Space.x8) {
        Text(state.title.uppercased())
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold)).tracking(0.8)
        Text("\(tasks.count)").font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .padding(.horizontal, MacTheme.Space.x7).padding(.vertical, MacTheme.Space.x1)
          .background(MacTheme.well, in: Capsule())
        Spacer()
        Image(systemName: "plus").font(.system(size: MacTheme.TypeScale.control, weight: .semibold))
          .foregroundStyle(MacTheme.muted)
      }.foregroundStyle(MacTheme.secondary).padding(.horizontal, MacTheme.Space.x4)
      ForEach(tasks, id: \.taskId) { task in
        TaskCard(
          task: task, ownerName: client.ownerName(of: task),
          commentCount: client.commentCount(of: task)
        )
        .onTapGesture { open(task) }.draggable(task.taskId)
      }
      Spacer(minLength: 40)
    }
    .taskColumnFrame()
    .dropDestination(for: String.self) { ids, _ in move(ids.first) }
  }

  private func move(_ taskId: String?) -> Bool {
    guard let taskId, let task = client.tasks.first(where: { $0.taskId == taskId }),
      task.state != state
    else { return false }
    client.moveTask(task, to: state, reason: ClientPolicy.taskMoveReason)
    return true
  }
}

struct TaskCard: View {
  let task: TaskRecord
  let ownerName: String?
  let commentCount: Int

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x7) {
      HStack(spacing: MacTheme.Space.x6) {
        Text(task.taskId).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.muted)
        Spacer()
        Text(task.priority.title.lowercased())
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.secondary).padding(.horizontal, MacTheme.Space.x7)
          .padding(.vertical, MacTheme.Space.x2).background(MacTheme.well, in: Capsule())
      }
      Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.ink).lineSpacing(3.9)
        .fixedSize(horizontal: false, vertical: true)
      HStack(spacing: MacTheme.Space.x6) {
        Circle().fill(indicatorColor).frame(
          width: DesignTokens.Layout.taskDot, height: DesignTokens.Layout.taskDot)
        Text(statusLine).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.secondary).lineLimit(2)
      }
      HStack(spacing: MacTheme.Space.x8) {
        if let ownerName {
          HStack(spacing: MacTheme.Space.x5) {
            Circle().fill(MacTheme.ink.opacity(0.75)).frame(
              width: DesignTokens.Layout.taskDot, height: DesignTokens.Layout.taskDot)
            Text(ownerName).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
              .lineLimit(1)
          }
          .foregroundStyle(MacTheme.ink).padding(.leading, MacTheme.Space.x6)
          .padding(.trailing, MacTheme.Space.x8).padding(.vertical, MacTheme.Space.x3)
          .background(ownerColor, in: Capsule())
        }
        if let branch = task.worktree?.branch {
          Label(branch, systemImage: "arrow.triangle.branch")
            .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium)).lineLimit(1)
        }
        Spacer()
        Label("\(commentCount)", systemImage: "message")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .semibold))
      }.foregroundStyle(MacTheme.muted)
    }
    .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
    .frame(maxWidth: .infinity, alignment: .leading)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
  }

  private var statusLine: String { TaskPresentation.statusLine(task) }

  private var indicatorColor: Color { MacTheme.color(for: TaskPresentation.indicator(task)) }

  private var ownerColor: Color {
    ownerName.map(taskOwnerColor) ?? MacTheme.well
  }
}
