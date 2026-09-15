import SoyehtClient
import SoyehtProtocol
import SwiftUI

private enum TaskViewMode: String, CaseIterable {
  case board = "Board"
  case list = "List"
}

private struct SelectedTask: Identifiable {
  let id: String
}

struct TaskWorkspaceView: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let onOpenPane: (String) -> Void
  @State private var mode = TaskViewMode.board
  @State private var showNewTask = false
  @State private var showAttention = false
  @State private var selectedTask: SelectedTask?

  private var tasks: [TaskRecord] { client.tasks(in: workspaceId) }
  private var attention: [AttentionItem] { client.attention(in: workspaceId) }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
      paneHeader
      VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
        toolbar
        if tasks.isEmpty {
          emptyState
        } else if mode == .board {
          TaskBoard(client: client, tasks: tasks, open: openTask)
        } else {
          TaskList(client: client, tasks: tasks, open: openTask)
        }
      }
      .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
    .padding(MacTheme.Space.x10)
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .padding(EdgeInsets(top: 8, leading: 6, bottom: 6, trailing: 6))
    .overlay(alignment: .top) {
      if showNewTask {
        ZStack(alignment: .top) {
          Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            .onTapGesture {}
          NewTaskSheet(client: client, workspaceId: workspaceId, isPresented: $showNewTask)
            .padding(.top, 47).transition(.scale(scale: 0.98).combined(with: .opacity))
        }.zIndex(20)
      }
    }
    .overlay(alignment: .topTrailing) {
      if let selection = selectedTask {
        TaskDetailSheet(client: client, workspaceId: workspaceId, taskId: selection.id) {
          selectedTask = nil
        }
        .padding(EdgeInsets(top: 8, leading: 0, bottom: 8, trailing: 12))
        .offset(y: -48)
        .transition(.move(edge: .trailing).combined(with: .opacity)).zIndex(10)
      }
    }
    .onAppear { client.refreshTasks(workspaceId) }
  }

  private var paneHeader: some View {
    HStack(spacing: MacTheme.Space.x7) {
      Image(systemName: "checklist").font(.system(size: 13, weight: .semibold))
      Text("tasks · \(workspaceName)")
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
      Spacer()
      Button {
        showAttention.toggle()
      } label: {
        Text(summary).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
      }
      .popover(isPresented: $showAttention, arrowEdge: .bottom) {
        AttentionPopover(
          items: attention, workspaceId: workspaceId, openPane: openAttentionPane)
      }
      Button {
        mode = .board
      } label: {
        Image(systemName: "rectangle.split.2x1")
      }
      .help("Board")
      Button {
        mode = .list
      } label: {
        Image(systemName: "rectangle.split.1x2")
      }
      .help("List")
      Image(systemName: "xmark")
    }
    .font(.system(size: 14, weight: .medium)).foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12).padding(.vertical, MacTheme.Space.x6)
    .background(MacTheme.well, in: Capsule())
  }

  private var toolbar: some View {
    HStack(spacing: MacTheme.Space.x12) {
      TaskViewToggle(mode: $mode)
      TaskToolbarLabel(
        "Filter", icon: "line.3.horizontal.decrease", trailingIcon: "chevron.down"
      ).accessibilityHidden(true)
      TaskToolbarLabel(
        "Priority", icon: "arrow.up.arrow.down", trailingIcon: "chevron.down"
      ).accessibilityHidden(true)
      if mode == .list {
        TaskToolbarLabel("Group: state", icon: "rectangle.3.group", trailingIcon: "chevron.down")
          .accessibilityHidden(true)
      }
      Spacer()
      dispatchMenu
      Button {
        showNewTask = true
      } label: {
        TaskToolbarLabel("New task", icon: "plus", trailingIcon: nil, accent: true)
      }.disabled(!client.connected).buttonStyle(.plain)
    }.frame(maxWidth: .infinity, minHeight: 34)
  }

  private var emptyState: some View {
    ContentUnavailableView {
      Label("No tasks", systemImage: "checklist")
    } description: {
      Text("Create a task for this workspace, then dispatch it or let an idle pane claim it.")
    } actions: {
      Button("New task") { showNewTask = true }.buttonStyle(NeoPillButtonStyle(accent: true))
    }.font(MacTheme.chrome(MacTheme.TypeScale.body))
      .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var summary: String {
    let active = tasks.filter { $0.state == .in_progress }.count
    return "\(tasks.count) tasks · \(active) in progress · \(attention.count) need you"
  }

  private var workspaceName: String {
    client.workspaces.first { $0.workspaceId == workspaceId }?.name ?? "Workspace"
  }

  private var dispatchMenu: some View {
    Menu {
      let ready = tasks.filter { $0.state == .todo && $0.ownerPaneId == nil }
      if ready.isEmpty {
        Text("No task is ready")
      } else {
        ForEach(ready, id: \.taskId) { task in
          Button("\(task.taskId) · \(task.title)") { client.dispatchTask(task) }
        }
      }
    } label: {
      TaskToolbarLabel("Dispatch", icon: "paperplane", trailingIcon: "chevron.down")
    }.menuIndicator(.hidden).buttonStyle(.plain).fixedSize().disabled(!client.connected)
  }

  private func openTask(_ task: TaskRecord) {
    client.refreshTask(workspaceId: task.workspaceId, taskId: task.taskId)
    selectedTask = SelectedTask(id: task.taskId)
  }

  private func openAttentionPane(_ paneId: String) {
    showAttention = false
    onOpenPane(paneId)
  }
}

private struct TaskViewToggle: View {
  @Binding var mode: TaskViewMode

  var body: some View {
    HStack(spacing: MacTheme.Space.x2) {
      choice(.board, icon: "square.grid.2x2")
      choice(.list, icon: "list.bullet")
    }
    .padding(MacTheme.Space.x3).background(MacTheme.well, in: Capsule())
  }

  private func choice(_ value: TaskViewMode, icon: String) -> some View {
    Button {
      mode = value
    } label: {
      Label(value.rawValue, systemImage: icon)
        .font(
          MacTheme.chrome(MacTheme.TypeScale.control, weight: mode == value ? .bold : .semibold)
        )
        .foregroundStyle(mode == value ? MacTheme.ink : MacTheme.muted)
        .padding(.horizontal, MacTheme.Space.x12).padding(.vertical, MacTheme.Space.x6)
        .background {
          if mode == value {
            RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.face)
              .shadow(color: MacTheme.shadowDark, radius: 2, x: 2, y: 2)
              .shadow(color: MacTheme.shadowLight, radius: 2, x: -2, y: -2)
          }
        }
    }.buttonStyle(.plain)
  }
}

private struct TaskToolbarLabel: View {
  let title: String
  let icon: String
  let trailingIcon: String?
  let accent: Bool

  init(_ title: String, icon: String, trailingIcon: String?, accent: Bool = false) {
    self.title = title
    self.icon = icon
    self.trailingIcon = trailingIcon
    self.accent = accent
  }

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: 13, weight: .semibold))
      Text(title).font(
        MacTheme.chrome(MacTheme.TypeScale.control, weight: accent ? .bold : .semibold))
      if let trailingIcon {
        Image(systemName: trailingIcon).font(.system(size: 10, weight: .semibold))
          .foregroundStyle(accent ? MacTheme.onAccent : MacTheme.muted)
      }
    }
    .foregroundStyle(accent ? MacTheme.onAccent : MacTheme.secondary)
    .padding(.leading, MacTheme.Space.x10).padding(.trailing, MacTheme.Space.x10)
    .padding(.vertical, MacTheme.Space.x6)
    .background {
      if accent {
        RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.accent)
          .shadow(color: MacTheme.accent.opacity(0.4), radius: 5, x: 4, y: 4)
          .shadow(color: MacTheme.shadowLight, radius: 3, x: -3, y: -3)
      } else {
        RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.face)
          .shadow(color: MacTheme.shadowDark, radius: 2.5, x: 3, y: 3)
          .shadow(color: MacTheme.shadowLight, radius: 2.5, x: -3, y: -3)
      }
    }
  }
}

private struct TaskBoard: View {
  @Bindable var client: EngineClient
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  private var visibleStates: [TaskState] {
    var states: [TaskState] = [.todo, .in_progress, .in_review, .completed]
    if tasks.contains(where: { $0.state == .cancelled }) { states.append(.cancelled) }
    return states
  }

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

private struct TaskColumn: View {
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
          .padding(.horizontal, MacTheme.Space.x7).padding(.vertical, 1)
          .background(MacTheme.well, in: Capsule())
        Spacer()
        Image(systemName: "plus").font(.system(size: 12, weight: .semibold))
          .foregroundStyle(MacTheme.muted)
      }.foregroundStyle(MacTheme.secondary).padding(.horizontal, MacTheme.Space.x4)
      ForEach(tasks, id: \.taskId) { task in
        TaskCard(
          task: task, ownerName: ownerName(task), commentCount: commentCount(task)
        )
        .onTapGesture { open(task) }.draggable(task.taskId)
      }
      Spacer(minLength: 40)
    }
    .frame(minWidth: 250, maxWidth: .infinity, minHeight: 500, alignment: .top)
    .dropDestination(for: String.self) { ids, _ in move(ids.first) }
  }

  private func ownerName(_ task: TaskRecord) -> String? {
    guard let owner = task.ownerPaneId else { return nil }
    return client.panes.first { $0.paneId == owner }?.name ?? owner
  }

  private func commentCount(_ task: TaskRecord) -> Int {
    client.taskDetails[task.taskId]?.comments.count ?? 0
  }

  private func move(_ taskId: String?) -> Bool {
    guard let taskId, let task = client.tasks.first(where: { $0.taskId == taskId }),
      task.state != state
    else { return false }
    client.moveTask(task, to: state, reason: "Moved on the workspace board")
    return true
  }
}

private struct TaskCard: View {
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
        Circle().fill(indicatorColor).frame(width: 6, height: 6)
        Text(statusLine).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.secondary).lineLimit(2)
      }
      HStack(spacing: MacTheme.Space.x8) {
        if let ownerName {
          HStack(spacing: MacTheme.Space.x5) {
            Circle().fill(MacTheme.ink.opacity(0.75)).frame(width: 6, height: 6)
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

  private var statusLine: String {
    task.reviewBadge.map { "\($0.title) · \(task.phrase)" } ?? task.phrase
  }

  private var indicatorColor: Color {
    if let badge = task.reviewBadge { return badge.color }
    if task.blocking != nil { return MacTheme.warning }
    return task.state.color
  }

  private var ownerColor: Color {
    let colors = [MacTheme.agentRose, MacTheme.agentBlue, MacTheme.agentGreen, MacTheme.agentSand]
    guard let ownerName else { return MacTheme.well }
    let value = ownerName.utf8.reduce(0) { ($0 + Int($1)) % colors.count }
    return colors[value]
  }
}

private struct TaskList: View {
  @Bindable var client: EngineClient
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  private var visibleStates: [TaskState] {
    var states: [TaskState] = [.in_progress, .in_review, .todo, .completed]
    if tasks.contains(where: { $0.state == .cancelled }) { states.append(.cancelled) }
    return states
  }

  var body: some View {
    GeometryReader { geometry in
      ScrollView([.horizontal, .vertical]) {
        LazyVStack(alignment: .leading, spacing: 0) {
          TaskListHeader()
          Rectangle().fill(MacTheme.well).frame(height: 1)
          ForEach(visibleStates, id: \.rawValue) { state in
            let rows = tasks.filter { $0.state == state }.sorted(by: taskOrder)
            if !rows.isEmpty {
              TaskListGroup(
                client: client, state: state, tasks: rows, open: open)
            }
          }
        }.frame(
          minWidth: max(960, geometry.size.width), minHeight: geometry.size.height,
          alignment: .topLeading)
      }.scrollIndicators(.never)
    }
  }

  private func taskOrder(_ left: TaskRecord, _ right: TaskRecord) -> Bool {
    let priorities: [TaskPriority] = [.urgent, .high, .medium, .low]
    let leftIndex = priorities.firstIndex(of: left.priority) ?? priorities.count
    let rightIndex = priorities.firstIndex(of: right.priority) ?? priorities.count
    if leftIndex != rightIndex { return leftIndex < rightIndex }
    return left.taskId < right.taskId
  }
}

private struct TaskListHeader: View {
  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      heading("KEY", width: 70)
      heading("TASK", fill: true)
      heading("STATE", width: 120)
      heading("PRIORITY", width: 90)
      heading("OWNER", width: 120)
      heading("WORKTREE", width: 90)
      heading("", width: 40)
      heading("UPDATED", width: 90)
    }.padding(EdgeInsets(top: 2, leading: 12, bottom: 6, trailing: 12))
  }

  private func heading(_ title: String, width: CGFloat? = nil, fill: Bool = false) -> some View {
    Text(title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold)).tracking(1)
      .foregroundStyle(MacTheme.muted)
      .frame(
        minWidth: fill ? 220 : nil, maxWidth: fill ? .infinity : width,
        alignment: .leading
      )
      .frame(width: fill ? nil : width, alignment: .leading)
  }
}

private struct TaskListGroup: View {
  @Bindable var client: EngineClient
  let state: TaskState
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 0) {
      HStack(spacing: MacTheme.Space.x8) {
        Image(systemName: "chevron.down").font(.system(size: 13, weight: .semibold))
        Text(state.title.uppercased())
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)).tracking(0.8)
        Text("\(tasks.count)").font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .padding(.horizontal, MacTheme.Space.x7).padding(.vertical, 1)
          .background(MacTheme.well, in: Capsule())
      }.foregroundStyle(MacTheme.muted)
        .padding(EdgeInsets(top: 8, leading: 8, bottom: 3, trailing: 12))
      ForEach(tasks, id: \.taskId) { task in
        TaskListRow(
          task: task, ownerName: ownerName(task), commentCount: commentCount(task), open: open)
        Rectangle().fill(MacTheme.well).frame(height: 1).padding(.horizontal, 12)
      }
    }
  }

  private func ownerName(_ task: TaskRecord) -> String? {
    guard let owner = task.ownerPaneId else { return nil }
    return client.panes.first { $0.paneId == owner }?.name ?? owner
  }

  private func commentCount(_ task: TaskRecord) -> Int {
    client.taskDetails[task.taskId]?.comments.count ?? 0
  }
}

private struct TaskListRow: View {
  let task: TaskRecord
  let ownerName: String?
  let commentCount: Int
  let open: (TaskRecord) -> Void
  @State private var hovered = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      Text(task.taskId).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.muted).frame(width: 70, alignment: .leading)
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .foregroundStyle(MacTheme.ink).lineLimit(1)
        HStack(spacing: MacTheme.Space.x5) {
          Circle().fill(task.state.color).frame(width: 5, height: 5)
          Text(task.phrase)
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.secondary).lineLimit(1)
        }
      }.frame(minWidth: 220, maxWidth: .infinity, alignment: .leading)
      TaskStateChip(state: task.state).frame(width: 120, alignment: .leading)
      TaskPriorityChip(priority: task.priority).frame(width: 90, alignment: .leading)
      TaskOwnerChip(name: ownerName).frame(width: 120, alignment: .leading)
      Text(task.worktree?.branch ?? "—")
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).lineLimit(1).frame(width: 90, alignment: .leading)
      Label("\(commentCount)", systemImage: "message")
        .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .semibold))
        .foregroundStyle(MacTheme.muted).frame(width: 40, alignment: .leading)
      Text("—").font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).frame(width: 90, alignment: .leading)
    }
    .padding(EdgeInsets(top: 5, leading: 12, bottom: 5, trailing: 12))
    .background(hovered ? MacTheme.well : Color.clear, in: RoundedRectangle(cornerRadius: 10))
    .contentShape(Rectangle()).onTapGesture { open(task) }
    .onHover { hovered = $0 }
  }
}

private struct TaskStateChip: View {
  let state: TaskState

  var body: some View {
    Text(state.title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(state == .in_progress ? MacTheme.onAccent : state.color)
      .padding(.horizontal, 9).padding(.vertical, 3)
      .background(
        state == .in_progress ? MacTheme.accent : state.color.opacity(0.14), in: Capsule())
  }
}

private struct TaskPriorityChip: View {
  let priority: TaskPriority

  var body: some View {
    Text(priority.title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(MacTheme.secondary).padding(.horizontal, 8).padding(.vertical, 2)
      .background(MacTheme.well, in: Capsule())
  }
}

private struct TaskOwnerChip: View {
  let name: String?

  var body: some View {
    if let name {
      HStack(spacing: MacTheme.Space.x5) {
        Circle().fill(MacTheme.ink.opacity(0.75)).frame(width: 6, height: 6)
        Text(name).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold)).lineLimit(1)
      }.foregroundStyle(MacTheme.ink).padding(.leading, 6).padding(.trailing, 8)
        .padding(.vertical, 3).background(taskOwnerColor(name), in: Capsule())
    } else {
      Text("—").font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
    }
  }
}

@MainActor private func taskOwnerColor(_ name: String) -> Color {
  let colors = [MacTheme.agentRose, MacTheme.agentBlue, MacTheme.agentGreen, MacTheme.agentSand]
  let value = name.utf8.reduce(0) { ($0 + Int($1)) % colors.count }
  return colors[value]
}

private struct AttentionPopover: View {
  let items: [AttentionItem]
  let workspaceId: String
  let openPane: (String) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("\(items.count) need you").font(.headline)
      if items.isEmpty {
        Text("Nothing needs your attention.").foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(items, id: \.attentionId) { item in
          VStack(alignment: .leading, spacing: 8) {
            FilePathText(item.message, workspaceId: workspaceId)
              .font(MacTheme.chrome(MacTheme.TypeScale.control))
              .fixedSize(horizontal: false, vertical: true)
            Button("Go to pane") { openPane(item.paneId) }.buttonStyle(.bordered)
          }.padding(12).neoWell(radius: MacTheme.Radius.compact)
        }
      }
    }.padding(20).frame(width: 340).background(MacTheme.face).foregroundStyle(MacTheme.ink)
  }
}

private struct TaskDetailSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let taskId: String
  let close: () -> Void

  private var detail: TaskShowResult? { client.taskDetails[taskId] }
  private var task: TaskRecord? { detail?.task ?? client.tasks.first { $0.taskId == taskId } }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      detailHeader
      if let task {
        ScrollView { detailContent(task) }.scrollIndicators(.never)
        commentPreview
        actionPreview
      } else {
        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .padding(EdgeInsets(top: 18, leading: 20, bottom: 18, trailing: 20))
    .frame(width: 460, height: 826, alignment: .topLeading)
    .foregroundStyle(MacTheme.ink).neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
  }

  private var detailHeader: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text(taskId).font(MacTheme.mono(MacTheme.TypeScale.control, weight: .bold))
        .foregroundStyle(MacTheme.muted)
      Spacer()
      Image(systemName: "arrow.up.left.and.arrow.down.right")
        .font(.system(size: 14, weight: .medium)).foregroundStyle(MacTheme.muted)
      Button(action: close) {
        Image(systemName: "xmark").font(.system(size: 14, weight: .medium))
          .foregroundStyle(MacTheme.muted)
      }.buttonStyle(.plain)
    }
  }

  private func detailContent(_ task: TaskRecord) -> some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
        .lineSpacing(4).fixedSize(horizontal: false, vertical: true)
      taskFacts(task)
      runnerFacts(task)
      DetailSection(title: "DESCRIPTION", text: task.description)
      protectedBoundary(task.mustNotChange)
      if let verify = task.howToVerify { DetailSection(title: "HOW TO VERIFY", text: verify) }
      if let blocking = task.blocking {
        DetailSection(
          title: "BLOCKING · \(blocking.kind.title.uppercased())", text: blocking.reason)
      }
      subitems
      attempts
      summary(task)
      driftReview
      timeline
    }.frame(maxWidth: .infinity, alignment: .leading)
  }

  private func taskFacts(_ task: TaskRecord) -> some View {
    HStack(spacing: MacTheme.Space.x8) {
      DetailStatePill(state: task.state)
      TaskPriorityChip(priority: task.priority)
      if let name = ownerName(task) { TaskOwnerChip(name: name) }
      if let badge = task.reviewBadge { DetailPill(text: badge.title, color: badge.color) }
    }
  }

  private func runnerFacts(_ task: TaskRecord) -> some View {
    HStack(spacing: MacTheme.Space.x8) {
      Label(runnerName(task), systemImage: "cpu")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.ink).padding(.horizontal, 10).padding(.vertical, 4)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
      if let branch = task.worktree?.branch {
        Label("\(branch) · attempt \(task.currentAttempt)", systemImage: "arrow.triangle.branch")
          .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
          .foregroundStyle(MacTheme.muted).lineLimit(1)
      }
    }
  }

  private func runnerName(_ task: TaskRecord) -> String {
    guard let executor = task.executor, !executor.workspaceDefault else {
      return "Workspace default"
    }
    return [executor.harness, executor.model, executor.effort]
      .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
  }

  private func ownerName(_ task: TaskRecord) -> String? {
    guard let owner = task.ownerPaneId else { return nil }
    return client.panes.first { $0.paneId == owner }?.name ?? owner
  }

  private func protectedBoundary(_ text: String) -> some View {
    DetailGroup(title: "MUST NOT CHANGE") {
      Text(text).font(MacTheme.chrome(MacTheme.TypeScale.control))
        .textSelection(.enabled).padding(10).frame(maxWidth: .infinity, alignment: .leading)
        .neoWell(radius: MacTheme.Radius.compact)
    }
  }

  private var subitems: some View {
    let values = detail?.subitems ?? []
    let done = values.filter { $0.state == .done }.count
    return DetailGroup(title: "SUBITEMS · \(done) OF \(values.count)") {
      if detail?.subitems.isEmpty != false {
        Text("No subitems.").font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(values, id: \.subitemId) { item in
          HStack(spacing: MacTheme.Space.x8) {
            Image(systemName: item.state == .done ? "checkmark.circle.fill" : "circle")
              .font(.system(size: 14)).foregroundStyle(
                item.state == .done ? MacTheme.success : MacTheme.muted)
            Text(item.text).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .medium))
              .foregroundStyle(item.state == .done ? MacTheme.muted : MacTheme.ink)
            Spacer()
            if let agent = item.agent {
              Text(agent).font(MacTheme.chrome(MacTheme.TypeScale.small))
                .foregroundStyle(MacTheme.muted)
            }
          }
        }
      }
    }
  }

  private var attempts: some View {
    DetailGroup(title: "ATTEMPTS") {
      ForEach(detail?.attempts ?? [], id: \.n) { attempt in
        VStack(alignment: .leading, spacing: 3) {
          Text("Attempt \(attempt.n) · \(attempt.harness) · \(attempt.model)").font(
            MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
          Text(attempt.result?.title ?? "active").font(
            MacTheme.chrome(MacTheme.TypeScale.small)
          ).foregroundStyle(MacTheme.secondary)
          if let value = attempt.summary {
            Text(value).font(MacTheme.chrome(MacTheme.TypeScale.small))
          }
        }
      }
    }
  }

  private func summary(_ task: TaskRecord) -> some View {
    DetailGroup(title: "SUMMARY") {
      Text(task.summary ?? "No summary yet.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(task.summary == nil ? MacTheme.secondary : MacTheme.ink)
    }
  }

  private var driftReview: some View {
    let lines = (detail?.comments ?? []).flatMap { comment in
      comment.text.split(separator: "\n").map(String.init).filter { $0.contains("→") }
    }
    return DetailGroup(title: "DRIFT REVIEW") {
      DeclarationList(
        title: "Contract changes",
        values: detail?.task.contractChanges?.map {
          "\($0.path)::\($0.name) · \($0.label.rawValue) · \($0.contractRef)"
        } ?? [])
      DeclarationList(
        title: "Out-of-scope edits",
        values: detail?.task.outOfScopeEdits?.map { "\($0.path) · \($0.reason)" } ?? [])
      if lines.isEmpty {
        Text("No reviewer findings.").font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
          FilePathText(line, workspaceId: workspaceId)
            .font(MacTheme.mono(MacTheme.TypeScale.small)).textSelection(.enabled)
        }
      }
    }
  }

  private var timeline: some View {
    DetailGroup(title: "HISTORY AND COMMENTS") {
      ForEach(timelineEntries) { entry in
        HStack(alignment: .top, spacing: MacTheme.Space.x8) {
          Image(systemName: entry.author == nil ? "circle.fill" : "person.crop.circle.fill")
            .font(.system(size: 10)).foregroundStyle(MacTheme.muted).padding(.top, 3)
          VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
            if let author = entry.author {
              Text(author).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
                .padding(.horizontal, 8).padding(.vertical, 2)
                .background(taskOwnerColor(author), in: Capsule())
            }
            FilePathText(entry.text, workspaceId: workspaceId)
              .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
              .foregroundStyle(entry.author == nil ? MacTheme.secondary : MacTheme.ink)
              .textSelection(.enabled)
            Text(entry.at).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
              .foregroundStyle(MacTheme.muted)
          }.frame(maxWidth: .infinity, alignment: .leading)
        }.padding(.vertical, 3)
      }
    }
  }

  private var timelineEntries: [TaskTimelineEntry] {
    let moves = (detail?.history ?? []).enumerated().map { index, move in
      TaskTimelineEntry(
        id: "move-\(index)-\(move.at)", author: nil,
        text: "\(move.actor) · \(move.from.title) → \(move.to.title) · \(move.reason)", at: move.at)
    }
    let comments = (detail?.comments ?? []).map { comment in
      TaskTimelineEntry(
        id: "comment-\(comment.commentId)", author: comment.author, text: comment.text,
        at: comment.at)
    }
    return (moves + comments).sorted { $0.at < $1.at }
  }

  private var commentPreview: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text("Comment…").font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.muted)
      Spacer()
      Image(systemName: "arrow.up").font(.system(size: 14, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
    }.padding(.horizontal, 14).frame(height: 44).neoWell(radius: MacTheme.Radius.control)
      .accessibilityHidden(true)
  }

  private var actionPreview: some View {
    HStack(spacing: MacTheme.Space.x10) {
      PreviewAction(title: "Dispatch to pane", icon: "paperplane", trailing: true)
      PreviewAction(title: "Send for review", icon: "eye", trailing: false)
      Spacer()
      Image(systemName: "ellipsis").font(.system(size: 16, weight: .medium))
        .foregroundStyle(MacTheme.muted)
    }.accessibilityHidden(true)
  }
}

private struct TaskTimelineEntry: Identifiable {
  let id: String
  let author: String?
  let text: String
  let at: String
}

private struct DetailStatePill: View {
  let state: TaskState

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Text(state.title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
      Image(systemName: "chevron.down").font(.system(size: 11, weight: .bold))
    }.foregroundStyle(MacTheme.onAccent).padding(.horizontal, 10).padding(.vertical, 4)
      .background(state.color, in: Capsule())
  }
}

private struct PreviewAction: View {
  let title: String
  let icon: String
  let trailing: Bool

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: icon).font(.system(size: 13, weight: .semibold))
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      if trailing {
        Image(systemName: "chevron.down").font(.system(size: 11, weight: .semibold))
          .foregroundStyle(MacTheme.muted)
      }
    }.foregroundStyle(MacTheme.ink).padding(.horizontal, 14).padding(.vertical, 8)
      .neoRaised(radius: MacTheme.Radius.pill, elevation: .card)
  }
}

private struct DetailPill: View {
  let text: String
  let color: Color
  var body: some View {
    Text(text).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .padding(.horizontal, 9).padding(.vertical, 5)
      .background(color.opacity(0.14), in: Capsule()).foregroundStyle(color)
  }
}

private struct DetailSection: View {
  let title: String
  let text: String
  var body: some View {
    DetailGroup(title: title) {
      Text(text).font(MacTheme.chrome(MacTheme.TypeScale.control)).textSelection(.enabled)
    }
  }
}

private struct DetailGroup<Content: View>: View {
  let title: String
  @ViewBuilder let content: Content
  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold)).tracking(1.1)
        .foregroundStyle(MacTheme.secondary)
      content
    }.frame(maxWidth: .infinity, alignment: .leading)
  }
}

private struct DeclarationList: View {
  let title: String
  let values: [String]
  var body: some View {
    VStack(alignment: .leading, spacing: 5) {
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
      if values.isEmpty {
        Text("None declared.").font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(values, id: \.self) { Text($0).font(MacTheme.mono(MacTheme.TypeScale.small)) }
      }
    }.padding(12).neoWell(radius: MacTheme.Radius.compact)
  }
}

private struct NewTaskSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  @Binding var isPresented: Bool
  @State private var title = ""
  @State private var description = ""
  @State private var mustNotChange = ""
  @State private var howToVerify = ""
  @State private var priority = TaskPriority.medium
  @State private var createWorktree = true
  @State private var permission = PermissionMode.ask
  @State private var workspaceDefault = true
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var dependencies: Set<String> = []
  @State private var showHowToVerify = false
  @State private var submitting = false

  private var valid: Bool {
    missingRequiredFields.isEmpty && (workspaceDefault || !harness.isEmpty)
  }
  private var missingRequiredFields: [String] {
    [
      ("title", title),
      ("description", description),
      ("MUST NOT CHANGE", mustNotChange),
    ].compactMap { name, value in
      value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? name : nil
    }
  }
  private var actionHint: String {
    if !missingRequiredFields.isEmpty {
      return "Required: \(missingRequiredFields.joined(separator: ", "))"
    }
    if !workspaceDefault && harness.isEmpty { return "Required: choose an agent" }
    return "⌘↩ create · ⇧⌘↩ create & dispatch"
  }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first { $0.category == "thought_level" || $0.id == "effort" }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      HStack(alignment: .top, spacing: MacTheme.Space.x8) {
        VStack(alignment: .leading, spacing: 3) {
          Text("New task").font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          Text("It enters this workspace list. An agent can claim it, or you can dispatch it.")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Button {
          isPresented = false
        } label: {
          Image(systemName: "xmark").font(.system(size: 14, weight: .medium))
            .foregroundStyle(MacTheme.muted)
        }.buttonStyle(.plain)
      }
      TaskField(title: "TITLE · REQUIRED", text: $title)
      TaskEditor(
        title: "DESCRIPTION · REQUIRED", text: $description, showsAccessories: true,
        accessoryAction: { showHowToVerify = true }
      )
      .popover(isPresented: $showHowToVerify, arrowEdge: .bottom) { verificationEditor }
      TaskEditor(title: "MUST NOT CHANGE · REQUIRED", text: $mustNotChange, showsAccessories: true)
      runnerOptions
      taskOptions
      actions
    }
    .padding(EdgeInsets(top: 18, leading: 22, bottom: 18, trailing: 22))
    .frame(width: 560).foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onAppear { client.refreshHarnesses() }
    .onExitCommand { isPresented = false }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      if !value.isEmpty { client.refreshCatalogue(harness: value) }
    }
  }

  private var runnerOptions: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: "WHO WILL RUN")
      HStack(spacing: MacTheme.Space.x10) {
        runnerMenu
        Spacer()
        WorkspaceDefaultToggle(isOn: $workspaceDefault)
      }.padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
    }
  }

  private var taskOptions: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        ForEach(TaskPriority.all, id: \.rawValue) { value in
          Button(value.title) { priority = value }
        }
      } label: {
        DialogOptionLabel(title: priority.title.lowercased(), icon: "flag")
      }
      Menu {
        Button(createWorktree ? "Use the workspace directly" : "Create a task worktree") {
          createWorktree.toggle()
        }
      } label: {
        DialogOptionLabel(
          title: createWorktree ? "worktree" : "workspace", icon: "arrow.triangle.branch")
      }
      Menu {
        Button("Ask every time") { permission = .ask }
        Button("Accept edits") { permission = .accept_edits }
        Button("Allow all actions") { permission = .allow_all }
      } label: {
        DialogOptionLabel(title: permission.dialogTitle, icon: "checkmark.shield")
      }
      Menu {
        ForEach(client.tasks(in: workspaceId), id: \.taskId) { task in
          Button {
            toggleDependency(task.taskId)
          } label: {
            Label(
              "\(task.taskId) · \(task.title)",
              systemImage: dependencies.contains(task.taskId) ? "checkmark" : "circle")
          }
        }
      } label: {
        DialogOptionLabel(title: dependencyTitle, icon: "link")
      }
    }
    .menuIndicator(.hidden).buttonStyle(.plain).fixedSize(horizontal: false, vertical: true)
  }

  private var runnerMenu: some View {
    Menu {
      Button("Workspace default") { workspaceDefault = true }
      Section("Agent") {
        ForEach(client.harnesses.filter(\.available), id: \.id) { value in
          Button(value.name) {
            workspaceDefault = false
            harness = value.id
          }
        }
      }
      if !workspaceDefault, !harness.isEmpty {
        Section("Model") {
          Button("Agent default") { model = "" }
          ForEach(catalogue?.models ?? [], id: \.id) { value in
            Button(value.name) { model = value.id }
          }
        }
        Section("Effort") {
          Button("Agent default") { effort = "" }
          ForEach(effortOption?.options ?? [], id: \.value) { value in
            Button(value.name) { effort = value.value }
          }
        }
      }
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Image(systemName: "cpu").font(.system(size: 14, weight: .medium))
        Text(runnerTitle).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .lineLimit(1)
        Image(systemName: "chevron.down").font(.system(size: 12, weight: .medium))
          .foregroundStyle(MacTheme.muted)
      }
      .foregroundStyle(MacTheme.ink).padding(.horizontal, 10).padding(.vertical, 6)
      .neoRaised(radius: MacTheme.Radius.pill, elevation: .taskCard)
    }.menuIndicator(.hidden).buttonStyle(.plain)
  }

  private var runnerTitle: String {
    guard !workspaceDefault else { return "Workspace default" }
    let harnessName = client.harnesses.first { $0.id == harness }?.name ?? "Choose an agent"
    let modelName = catalogue?.models.first { $0.id == model }?.name
    let effortName = effortOption?.options.first { $0.value == effort }?.name
    return [harnessName, modelName, effortName].compactMap { $0 }.joined(separator: " · ")
  }

  private var dependencyTitle: String {
    guard let first = dependencies.sorted().first else { return "dependencies" }
    return dependencies.count == 1 ? "after \(first)" : "after \(first) +\(dependencies.count - 1)"
  }

  private var verificationEditor: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text("HOW TO VERIFY · OPTIONAL").font(
        MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)
      ).tracking(1).foregroundStyle(MacTheme.muted)
      TextEditor(text: $howToVerify).font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .scrollContentBackground(.hidden).padding(10).frame(height: 112)
        .neoWell(radius: MacTheme.Radius.control)
    }.padding(18).frame(width: 360).background(MacTheme.face)
  }

  private var actions: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Text(actionHint)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
        .foregroundStyle(valid ? MacTheme.muted : MacTheme.warning)
      Spacer()
      Button("Create in list") { create(dispatch: false) }
        .buttonStyle(NeoPillButtonStyle(elevation: .action)).disabled(!valid || submitting)
        .accessibilityHint(valid ? "Create this task" : actionHint)
        .keyboardShortcut(.return, modifiers: [.command])
      Button {
        create(dispatch: true)
      } label: {
        Label("Create and dispatch", systemImage: "paperplane")
      }
      .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(!valid || submitting)
      .accessibilityHint(valid ? "Create and dispatch this task" : actionHint)
      .keyboardShortcut(.return, modifiers: [.command, .shift])
    }.padding(.top, MacTheme.Space.x6)
  }

  private func toggleDependency(_ taskId: String) {
    if dependencies.contains(taskId) {
      dependencies.remove(taskId)
    } else {
      dependencies.insert(taskId)
    }
  }

  private func create(dispatch: Bool) {
    guard valid, !submitting else { return }
    submitting = true
    let executor = TaskExecutor(
      workspaceDefault: workspaceDefault,
      harness: workspaceDefault ? nil : harness,
      model: model.nilIfEmpty, effort: effort.nilIfEmpty, permission: permission)
    Task { @MainActor in
      do {
        _ = try await client.createTask(
          workspaceId: workspaceId, title: title, description: description,
          howToVerify: howToVerify.nilIfEmpty, mustNotChange: mustNotChange,
          executor: executor, priority: priority, createWorktree: createWorktree,
          permission: permission, dependencies: dependencies.sorted(), dispatch: dispatch)
        isPresented = false
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct TaskField: View {
  let title: String
  @Binding var text: String

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: title)
      TextField("", text: $text).font(MacTheme.chrome(14, weight: .semibold))
        .textFieldStyle(.plain).padding(.horizontal, 14).frame(height: 40)
        .neoWell(radius: MacTheme.Radius.control)
    }
  }
}

private struct TaskEditor: View {
  let title: String
  @Binding var text: String
  var showsAccessories = false
  var accessoryAction: (() -> Void)? = nil

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: title)
      ZStack(alignment: .bottomLeading) {
        TextEditor(text: $text).font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .scrollContentBackground(.hidden)
          .padding(
            EdgeInsets(
              top: 8, leading: 10, bottom: showsAccessories ? 28 : 8, trailing: 10))
        if showsAccessories { editorAccessories }
      }.frame(height: 96).neoWell(radius: MacTheme.Radius.control)
    }
  }

  private var editorAccessories: some View {
    HStack(spacing: MacTheme.Space.x8) {
      if let accessoryAction {
        Button(action: accessoryAction) {
          Image(systemName: "paperclip").font(.system(size: 14, weight: .medium))
        }.buttonStyle(.plain).help("Edit verification notes")
          .accessibilityLabel("Edit verification notes")
      } else {
        Image(systemName: "paperclip").font(.system(size: 14, weight: .medium))
      }
      Image(systemName: "at").font(.system(size: 14, weight: .medium))
    }.foregroundStyle(MacTheme.muted).padding(.leading, 14).padding(.bottom, 10)
      .accessibilityHidden(accessoryAction == nil)
  }
}

private struct DialogFieldLabel: View {
  let title: String

  var body: some View {
    Text(title).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)).tracking(1)
      .foregroundStyle(MacTheme.muted)
  }
}

private struct WorkspaceDefaultToggle: View {
  @Binding var isOn: Bool

  var body: some View {
    Button {
      isOn.toggle()
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Text("Use workspace default")
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        ZStack {
          Capsule().fill(MacTheme.well).frame(width: 36, height: 20)
          Color.clear.frame(width: 16, height: 16)
            .neoRaised(radius: MacTheme.Radius.pill, elevation: .toggle)
            .offset(x: isOn ? 8 : -8)
        }.frame(width: 36, height: 20)
      }.foregroundStyle(MacTheme.secondary)
    }
    .buttonStyle(.plain).accessibilityValue(isOn ? "On" : "Off")
  }
}

private struct DialogOptionLabel: View {
  let title: String
  let icon: String

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: 12, weight: .medium))
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      Image(systemName: "chevron.down").font(.system(size: 11, weight: .medium))
        .foregroundStyle(MacTheme.muted)
    }
    .foregroundStyle(MacTheme.secondary).padding(.horizontal, 10).padding(.vertical, 6)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
  }
}

extension String {
  fileprivate var nilIfEmpty: String? {
    let value = trimmingCharacters(in: .whitespacesAndNewlines)
    return value.isEmpty ? nil : value
  }
}

extension PermissionMode {
  fileprivate var dialogTitle: String {
    switch self {
    case .ask: "permission"
    case .accept_edits: "accept edits"
    case .allow_all: "allow all"
    }
  }
}

@MainActor extension TaskState {
  fileprivate var title: String {
    switch self {
    case .todo: "To do"
    case .in_progress: "In progress"
    case .in_review: "In review"
    case .completed: "Completed"
    case .cancelled: "Cancelled"
    }
  }
  fileprivate var color: Color {
    switch self {
    case .todo: MacTheme.secondary
    case .in_progress: MacTheme.accent
    case .in_review: MacTheme.warning
    case .completed: MacTheme.success
    case .cancelled: MacTheme.urgent
    }
  }
}

@MainActor extension TaskPriority {
  fileprivate static let all: [TaskPriority] = [.urgent, .high, .medium, .low]
  fileprivate var title: String { rawValue.replacingOccurrences(of: "_", with: " ").capitalized }
  fileprivate var color: Color {
    switch self {
    case .urgent: MacTheme.urgent
    case .high: MacTheme.warning
    case .medium: MacTheme.accent
    case .low: MacTheme.secondary
    }
  }
}

@MainActor extension TaskReviewBadge {
  fileprivate var title: String { self == .your_turn ? "YOUR TURN" : "SYSTEM'S TURN" }
  fileprivate var color: Color { self == .your_turn ? MacTheme.accent : MacTheme.warning }
}

extension BlockingKind {
  fileprivate var title: String { rawValue.replacingOccurrences(of: "_", with: " ") }
}

extension AttemptResult {
  fileprivate var title: String { rawValue.capitalized }
}
