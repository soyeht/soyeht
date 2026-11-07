import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct TaskList: View {
  @Bindable var client: EngineClient
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  private var visibleStates: [TaskState] { TaskPresentation.listStates(tasks) }

  var body: some View {
    GeometryReader { geometry in
      ScrollView([.horizontal, .vertical]) {
        LazyVStack(alignment: .leading, spacing: 0) {
          TaskListHeader()
          Rectangle().fill(MacTheme.well).frame(height: 1)
          ForEach(visibleStates, id: \.rawValue) { state in
            let rows = tasks.filter { $0.state == state }.sorted(by: TaskPresentation.listPrecedes)
            if !rows.isEmpty {
              TaskListGroup(
                client: client, state: state, tasks: rows, open: open)
            }
          }
        }.frame(
          minWidth: max(DesignTokens.Layout.taskListMinimumWidth, geometry.size.width),
          minHeight: geometry.size.height,
          alignment: .topLeading)
      }.scrollIndicators(.never)
    }
  }

}

struct TaskListHeader: View {
  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      heading("KEY", width: DesignTokens.Layout.taskListKeyWidth)
      heading("TASK", fill: true)
      heading("STATE", width: DesignTokens.Layout.taskListStateWidth)
      heading("PRIORITY", width: DesignTokens.Layout.taskListPriorityWidth)
      heading("OWNER", width: DesignTokens.Layout.taskListOwnerWidth)
      heading("WORKTREE", width: DesignTokens.Layout.taskListWorktreeWidth)
      heading("", width: DesignTokens.Layout.taskListCommentsWidth)
      heading("UPDATED", width: DesignTokens.Layout.taskListUpdatedWidth)
    }.padding(EdgeInsets(top: 2, leading: 12, bottom: 6, trailing: 12))
  }

  private func heading(_ title: String, width: CGFloat? = nil, fill: Bool = false) -> some View {
    Text(title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold)).tracking(1)
      .foregroundStyle(MacTheme.muted)
      .frame(
        minWidth: fill ? DesignTokens.Layout.taskListTitleMinimumWidth : nil,
        maxWidth: fill ? .infinity : width,
        alignment: .leading
      )
      .frame(width: fill ? nil : width, alignment: .leading)
  }
}

struct TaskListGroup: View {
  @Bindable var client: EngineClient
  let state: TaskState
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack(spacing: MacTheme.Space.x8) {
        Image(systemName: "chevron.down").font(
          .system(size: MacTheme.TypeScale.caption, weight: .semibold))
        Text(state.title.uppercased())
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)).tracking(0.8)
        Text("\(tasks.count)").font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .padding(.horizontal, MacTheme.Space.x7).padding(.vertical, MacTheme.Space.x1)
          .background(MacTheme.well, in: Capsule())
      }.foregroundStyle(MacTheme.muted)
        .padding(EdgeInsets(top: 8, leading: 8, bottom: 3, trailing: 12))
      ForEach(tasks, id: \.taskId) { task in
        TaskListRow(
          task: task, ownerName: client.ownerName(of: task),
          commentCount: client.commentCount(of: task), open: open)
        Rectangle().fill(MacTheme.well).frame(height: 1).padding(.horizontal, MacTheme.Space.x12)
      }
    }
  }
}

struct TaskListRow: View {
  let task: TaskRecord
  let ownerName: String?
  let commentCount: Int
  let open: (TaskRecord) -> Void
  @State private var hovered = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      Text(task.taskId).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.muted).frame(
          width: DesignTokens.Layout.taskListKeyWidth, alignment: .leading)
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .foregroundStyle(MacTheme.ink).lineLimit(1)
        HStack(spacing: MacTheme.Space.x5) {
          Circle().fill(task.state.color).frame(width: 5, height: 5)
          Text(task.phrase)
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.secondary).lineLimit(1)
        }
      }.frame(
        minWidth: DesignTokens.Layout.taskListTitleMinimumWidth, maxWidth: .infinity,
        alignment: .leading)
      TaskStateChip(state: task.state).frame(
        width: DesignTokens.Layout.taskListStateWidth, alignment: .leading)
      TaskPriorityChip(priority: task.priority).frame(
        width: DesignTokens.Layout.taskListPriorityWidth, alignment: .leading)
      TaskOwnerChip(name: ownerName).frame(
        width: DesignTokens.Layout.taskListOwnerWidth, alignment: .leading)
      Text(task.worktree?.branch ?? "—")
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).lineLimit(1).frame(
          width: DesignTokens.Layout.taskListWorktreeWidth, alignment: .leading)
      Label("\(commentCount)", systemImage: "message")
        .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .semibold))
        .foregroundStyle(MacTheme.muted).frame(
          width: DesignTokens.Layout.taskListCommentsWidth, alignment: .leading)
      Text("—").font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).frame(
          width: DesignTokens.Layout.taskListUpdatedWidth, alignment: .leading)
    }
    .padding(EdgeInsets(top: 5, leading: 12, bottom: 5, trailing: 12))
    .background(
      hovered ? MacTheme.well : Color.clear,
      in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
    )
    .contentShape(Rectangle()).onTapGesture { open(task) }
    .onHover { hovered = $0 }
  }
}
