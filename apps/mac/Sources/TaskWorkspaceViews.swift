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
    VStack(alignment: .leading, spacing: 0) {
      header
      Divider().overlay(MacTheme.canvas)
      toolbar
      if tasks.isEmpty {
        emptyState
      } else if mode == .board {
        TaskBoard(
          client: client, tasks: tasks,
          open: openTask)
      } else {
        TaskList(client: client, tasks: tasks, open: openTask)
      }
    }
    .background(MacTheme.face)
    .sheet(isPresented: $showNewTask) {
      NewTaskSheet(
        client: client, workspaceId: workspaceId, isPresented: $showNewTask)
    }
    .sheet(item: $selectedTask) { selection in
      TaskDetailSheet(client: client, taskId: selection.id)
    }
    .onAppear { client.refreshTasks(workspaceId) }
  }

  private var header: some View {
    HStack(spacing: 12) {
      Image(systemName: "list.bullet.clipboard").foregroundStyle(MacTheme.accent)
      VStack(alignment: .leading, spacing: 2) {
        Text("Workspace tasks").font(.system(size: 15, weight: .semibold, design: .rounded))
        Text(summary).font(.system(size: 11)).foregroundStyle(MacTheme.secondary)
      }
      Spacer()
      Button {
        showAttention.toggle()
      } label: {
        Label("\(attention.count) need you", systemImage: "bell")
          .font(.system(size: 12, weight: .semibold)).padding(.horizontal, 12).padding(.vertical, 8)
          .background(
            attention.isEmpty ? MacTheme.canvas : MacTheme.warning.opacity(0.16), in: Capsule())
      }
      .popover(isPresented: $showAttention, arrowEdge: .bottom) {
        AttentionPopover(items: attention, openPane: openAttentionPane)
      }
      Button {
        client.refreshTasks(workspaceId)
      } label: {
        Image(systemName: "arrow.clockwise").padding(7)
      }.help("Refresh tasks")
    }.padding(.horizontal, 20).padding(.vertical, 14)
  }

  private var toolbar: some View {
    HStack(spacing: 12) {
      Picker("View", selection: $mode) {
        ForEach(TaskViewMode.allCases, id: \.self) { Text($0.rawValue).tag($0) }
      }.pickerStyle(.segmented).frame(width: 180)
      Spacer()
      Button {
        showNewTask = true
      } label: {
        Label("New task", systemImage: "plus")
          .font(.system(size: 12, weight: .semibold)).padding(.horizontal, 14).padding(.vertical, 9)
          .neoRaised(radius: 11, elevation: .pill)
      }.disabled(!client.connected)
    }.padding(.horizontal, 20).padding(.vertical, 12)
  }

  private var emptyState: some View {
    ContentUnavailableView {
      Label("No tasks", systemImage: "checklist")
    } description: {
      Text("Create a task for this workspace, then dispatch it or let an idle pane claim it.")
    } actions: {
      Button("New task") { showNewTask = true }.buttonStyle(.borderedProminent)
    }.frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var summary: String {
    let active = tasks.filter { $0.state == .in_progress }.count
    return "\(tasks.count) tasks · \(active) in progress · \(attention.count) need you"
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
      HStack(alignment: .top, spacing: 14) {
        ForEach(visibleStates, id: \.rawValue) { state in
          TaskColumn(
            client: client, state: state, tasks: tasks.filter { $0.state == state }, open: open)
        }
      }.padding(20)
    }
  }
}

private struct TaskColumn: View {
  @Bindable var client: EngineClient
  let state: TaskState
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      HStack {
        Text(state.title.uppercased()).font(.system(size: 10, weight: .bold)).tracking(1.1)
        Text("\(tasks.count)").font(.system(size: 10, weight: .bold)).padding(5)
          .background(MacTheme.canvas, in: Circle())
        Spacer()
      }.foregroundStyle(MacTheme.secondary)
      ForEach(tasks, id: \.taskId) { task in
        TaskCard(task: task, dispatch: { client.dispatchTask(task) })
          .onTapGesture { open(task) }.draggable(task.taskId)
      }
      Spacer(minLength: 40)
    }
    .padding(14).frame(width: 224).frame(minHeight: 500, alignment: .top)
    .background(MacTheme.canvas.opacity(0.62), in: RoundedRectangle(cornerRadius: 16))
    .dropDestination(for: String.self) { ids, _ in move(ids.first) }
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
  let dispatch: () -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      HStack {
        Text(task.taskId).font(.system(size: 10, weight: .bold, design: .monospaced))
        Spacer()
        Text(task.priority.title).font(.system(size: 10, weight: .bold))
          .foregroundStyle(task.priority.color)
      }
      Text(task.title).font(.system(size: 13, weight: .semibold, design: .rounded))
        .fixedSize(horizontal: false, vertical: true)
      HStack(spacing: 7) {
        Circle().fill(task.state.color).frame(width: 7, height: 7)
        Text(task.phrase).font(.system(size: 11)).foregroundStyle(MacTheme.secondary)
          .lineLimit(2)
      }
      if let badge = task.reviewBadge {
        Text(badge.title).font(.system(size: 9, weight: .bold)).tracking(0.8)
          .padding(.horizontal, 8).padding(.vertical, 4)
          .background(badge.color.opacity(0.15), in: Capsule()).foregroundStyle(badge.color)
      }
      HStack(spacing: 8) {
        if let owner = task.ownerPaneId {
          Label(owner, systemImage: "person.crop.circle").lineLimit(1)
        }
        if let branch = task.worktree?.branch {
          Label(branch, systemImage: "arrow.triangle.branch").lineLimit(1)
        }
        Spacer()
        if task.state == .todo && task.ownerPaneId == nil {
          Button(action: dispatch) { Image(systemName: "paperplane.fill") }
            .help("Dispatch task")
        }
      }.font(.system(size: 9)).foregroundStyle(MacTheme.secondary)
    }.padding(13).neoRaised(radius: 13)
  }
}

private struct TaskList: View {
  @Bindable var client: EngineClient
  let tasks: [TaskRecord]
  let open: (TaskRecord) -> Void

  var body: some View {
    ScrollView {
      LazyVStack(spacing: 10) {
        ForEach(tasks.sorted(by: taskOrder), id: \.taskId) { task in
          HStack(spacing: 14) {
            Circle().fill(task.state.color).frame(width: 8, height: 8)
            Text(task.taskId).font(.system(size: 11, weight: .bold, design: .monospaced))
              .frame(width: 62, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
              Text(task.title).font(.system(size: 13, weight: .semibold))
              Text(task.phrase).font(.system(size: 10)).foregroundStyle(MacTheme.secondary)
            }
            Spacer()
            Text(task.priority.title).font(.system(size: 10, weight: .bold))
              .foregroundStyle(task.priority.color)
            Text(task.state.title).font(.system(size: 11)).foregroundStyle(MacTheme.secondary)
            if task.state == .todo && task.ownerPaneId == nil {
              Button("Dispatch") { client.dispatchTask(task) }.buttonStyle(.bordered)
            }
            Button("Open") { open(task) }.buttonStyle(.bordered)
          }.padding(13).neoRaised(radius: MacTheme.Radius.control)
        }
      }.padding(20)
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

private struct AttentionPopover: View {
  let items: [AttentionItem]
  let openPane: (String) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: 14) {
      Text("\(items.count) need you").font(.headline)
      if items.isEmpty {
        Text("Nothing needs your attention.").foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(items, id: \.attentionId) { item in
          VStack(alignment: .leading, spacing: 8) {
            Text(item.message).font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
            Button("Go to pane") { openPane(item.paneId) }.buttonStyle(.bordered)
          }.padding(12).neoWell(radius: MacTheme.Radius.compact)
        }
      }
    }.padding(20).frame(width: 340).background(MacTheme.face).foregroundStyle(MacTheme.ink)
  }
}

private struct TaskDetailSheet: View {
  @Bindable var client: EngineClient
  let taskId: String
  @Environment(\.dismiss) private var dismiss

  private var detail: TaskShowResult? { client.taskDetails[taskId] }
  private var task: TaskRecord? { detail?.task ?? client.tasks.first { $0.taskId == taskId } }

  var body: some View {
    VStack(spacing: 0) {
      HStack {
        Text(taskId).font(.system(size: 12, weight: .bold, design: .monospaced))
        Spacer()
        Button {
          dismiss()
        } label: {
          Image(systemName: "xmark").padding(6)
        }
      }.padding(20)
      Divider()
      if let task {
        ScrollView { detailContent(task) }
      } else {
        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }.frame(width: 620, height: 760).background(MacTheme.face).foregroundStyle(MacTheme.ink)
  }

  private func detailContent(_ task: TaskRecord) -> some View {
    VStack(alignment: .leading, spacing: 20) {
      VStack(alignment: .leading, spacing: 10) {
        Text(task.title).font(.system(size: 23, weight: .semibold, design: .rounded))
        HStack {
          DetailPill(text: task.state.title, color: task.state.color)
          DetailPill(text: task.priority.title, color: task.priority.color)
          if let badge = task.reviewBadge { DetailPill(text: badge.title, color: badge.color) }
        }
        Text(task.phrase).font(.system(size: 12)).foregroundStyle(MacTheme.secondary)
      }
      DetailSection(title: "DESCRIPTION", text: task.description)
      DetailSection(title: "MUST NOT CHANGE", text: task.mustNotChange)
      if let verify = task.howToVerify { DetailSection(title: "HOW TO VERIFY", text: verify) }
      if let blocking = task.blocking {
        DetailSection(
          title: "BLOCKING · \(blocking.kind.title.uppercased())", text: blocking.reason)
      }
      subitems
      attempts
      summary(task)
      driftReview
      comments
      history
    }.padding(24)
  }

  private var subitems: some View {
    DetailGroup(title: "SUBITEMS") {
      if detail?.subitems.isEmpty != false {
        Text("No subitems.").foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(detail?.subitems ?? [], id: \.subitemId) { item in
          Label(item.text, systemImage: item.state == .done ? "checkmark.circle.fill" : "circle")
            .foregroundStyle(item.state == .done ? MacTheme.success : MacTheme.ink)
        }
      }
    }
  }

  private var attempts: some View {
    DetailGroup(title: "ATTEMPTS") {
      ForEach(detail?.attempts ?? [], id: \.n) { attempt in
        VStack(alignment: .leading, spacing: 3) {
          Text("Attempt \(attempt.n) · \(attempt.harness) · \(attempt.model)").font(
            .system(size: 12, weight: .semibold))
          Text(attempt.result?.title ?? "active").font(.system(size: 11)).foregroundStyle(
            MacTheme.secondary)
          if let value = attempt.summary { Text(value).font(.system(size: 11)) }
        }
      }
    }
  }

  private func summary(_ task: TaskRecord) -> some View {
    DetailGroup(title: "SUMMARY") {
      Text(task.summary ?? "No summary yet.").foregroundStyle(
        task.summary == nil ? MacTheme.secondary : MacTheme.ink)
      DeclarationList(
        title: "Contract changes",
        values: task.contractChanges?.map {
          "\($0.path)::\($0.name) · \($0.label.rawValue) · \($0.contractRef)"
        } ?? [])
      DeclarationList(
        title: "Out-of-scope edits",
        values: task.outOfScopeEdits?.map { "\($0.path) · \($0.reason)" } ?? [])
    }
  }

  private var driftReview: some View {
    let lines = (detail?.comments ?? []).flatMap { comment in
      comment.text.split(separator: "\n").map(String.init).filter { $0.contains("→") }
    }
    return DetailGroup(title: "DRIFT REVIEW") {
      if lines.isEmpty {
        Text("No reviewer findings.").foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
          Text(line).font(.system(size: 11, design: .monospaced)).textSelection(.enabled)
        }
      }
    }
  }

  private var comments: some View {
    DetailGroup(title: "COMMENTS") {
      ForEach(detail?.comments ?? [], id: \.commentId) { comment in
        VStack(alignment: .leading, spacing: 3) {
          Text(comment.author).font(.system(size: 11, weight: .bold))
          Text(comment.text).font(.system(size: 12)).textSelection(.enabled)
          Text(comment.at).font(.system(size: 9)).foregroundStyle(MacTheme.secondary)
        }.padding(.vertical, 4)
      }
    }
  }

  private var history: some View {
    DetailGroup(title: "HISTORY") {
      ForEach(Array((detail?.history ?? []).enumerated()), id: \.offset) { _, move in
        Text("\(move.actor) · \(move.from.title) → \(move.to.title) · \(move.reason)")
          .font(.system(size: 11))
      }
    }
  }
}

private struct DetailPill: View {
  let text: String
  let color: Color
  var body: some View {
    Text(text).font(.system(size: 10, weight: .bold)).padding(.horizontal, 9).padding(.vertical, 5)
      .background(color.opacity(0.14), in: Capsule()).foregroundStyle(color)
  }
}

private struct DetailSection: View {
  let title: String
  let text: String
  var body: some View {
    DetailGroup(title: title) { Text(text).font(.system(size: 12)).textSelection(.enabled) }
  }
}

private struct DetailGroup<Content: View>: View {
  let title: String
  @ViewBuilder let content: Content
  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      Text(title).font(.system(size: 10, weight: .bold)).tracking(1.1).foregroundStyle(
        MacTheme.secondary)
      content
    }.frame(maxWidth: .infinity, alignment: .leading)
  }
}

private struct DeclarationList: View {
  let title: String
  let values: [String]
  var body: some View {
    VStack(alignment: .leading, spacing: 5) {
      Text(title).font(.system(size: 11, weight: .semibold))
      if values.isEmpty {
        Text("None declared.").font(.system(size: 11)).foregroundStyle(MacTheme.secondary)
      } else {
        ForEach(values, id: \.self) { Text($0).font(.system(size: 11, design: .monospaced)) }
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

  private var valid: Bool {
    !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && !description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && !mustNotChange.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && (workspaceDefault || !harness.isEmpty)
  }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first { $0.category == "thought_level" || $0.id == "effort" }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: 18) {
      HStack {
        VStack(alignment: .leading, spacing: 3) {
          Text("New task").font(.system(size: 24, weight: .semibold, design: .rounded))
          Text("Add it to the workspace list, or create and dispatch it now.")
            .font(.system(size: 12)).foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Button {
          isPresented = false
        } label: {
          Image(systemName: "xmark").padding(6)
        }
      }
      ScrollView {
        VStack(alignment: .leading, spacing: 14) {
          TaskField(title: "TITLE", text: $title)
          TaskEditor(title: "DESCRIPTION", text: $description)
          TaskEditor(title: "MUST NOT CHANGE · REQUIRED", text: $mustNotChange)
          TaskField(title: "HOW TO VERIFY · OPTIONAL", text: $howToVerify)
          runnerOptions
          taskOptions
        }.padding(4)
      }
      HStack {
        Text("⌘↩ creates · ⇧⌘↩ creates and dispatches")
          .font(.system(size: 10)).foregroundStyle(MacTheme.secondary)
        Spacer()
        Button("Create in list") { create(dispatch: false) }.buttonStyle(.bordered).disabled(!valid)
          .keyboardShortcut(.return, modifiers: [.command])
        Button("Create and dispatch") { create(dispatch: true) }.buttonStyle(.borderedProminent)
          .disabled(!valid).keyboardShortcut(.return, modifiers: [.command, .shift])
      }
    }.padding(26).frame(width: 640, height: 760).background(MacTheme.canvas).foregroundStyle(
      MacTheme.ink
    )
    .onAppear { client.refreshHarnesses() }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      if !value.isEmpty { client.refreshCatalogue(harness: value) }
    }
  }

  private var runnerOptions: some View {
    DetailGroup(title: "WHO WILL RUN") {
      Toggle("Use workspace default", isOn: $workspaceDefault)
      if !workspaceDefault {
        Picker("Agent", selection: $harness) {
          Text("Choose an agent").tag("")
          ForEach(client.harnesses.filter(\.available), id: \.id) { Text($0.name).tag($0.id) }
        }
        Picker("Model", selection: $model) {
          Text("Agent default").tag("")
          ForEach(catalogue?.models ?? [], id: \.id) { Text($0.name).tag($0.id) }
        }
        Picker("Effort", selection: $effort) {
          Text("Agent default").tag("")
          ForEach(effortOption?.options ?? [], id: \.value) { Text($0.name).tag($0.value) }
        }
      }
    }
  }

  private var taskOptions: some View {
    VStack(alignment: .leading, spacing: 12) {
      Picker("Priority", selection: $priority) {
        ForEach(TaskPriority.all, id: \.rawValue) { Text($0.title).tag($0) }
      }
      Toggle("Create a task worktree", isOn: $createWorktree)
      Picker("Permission", selection: $permission) {
        Text("Ask every time").tag(PermissionMode.ask)
        Text("Accept edits").tag(PermissionMode.accept_edits)
        Text("Allow all actions").tag(PermissionMode.allow_all)
      }
      Menu("Dependencies · \(dependencies.count)") {
        ForEach(client.tasks(in: workspaceId), id: \.taskId) { task in
          Button {
            toggleDependency(task.taskId)
          } label: {
            Label(
              "\(task.taskId) · \(task.title)",
              systemImage: dependencies.contains(task.taskId) ? "checkmark" : "circle")
          }
        }
      }
    }.pickerStyle(.menu)
  }

  private func toggleDependency(_ taskId: String) {
    if dependencies.contains(taskId) {
      dependencies.remove(taskId)
    } else {
      dependencies.insert(taskId)
    }
  }

  private func create(dispatch: Bool) {
    let executor = TaskExecutor(
      workspaceDefault: workspaceDefault,
      harness: workspaceDefault ? nil : harness,
      model: model.nilIfEmpty, effort: effort.nilIfEmpty, permission: permission)
    client.createTask(
      workspaceId: workspaceId, title: title, description: description,
      howToVerify: howToVerify.nilIfEmpty, mustNotChange: mustNotChange,
      executor: executor, priority: priority, createWorktree: createWorktree,
      permission: permission, dependencies: dependencies.sorted(), dispatch: dispatch)
    isPresented = false
  }
}

private struct TaskField: View {
  let title: String
  @Binding var text: String
  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      Text(title).font(.system(size: 10, weight: .bold)).tracking(1).foregroundStyle(
        MacTheme.secondary)
      TextField("", text: $text).textFieldStyle(.plain).padding(12).neoWell(
        radius: MacTheme.Radius.compact)
    }
  }
}

private struct TaskEditor: View {
  let title: String
  @Binding var text: String
  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      Text(title).font(.system(size: 10, weight: .bold)).tracking(1).foregroundStyle(
        MacTheme.secondary)
      TextEditor(text: $text).scrollContentBackground(.hidden).frame(minHeight: 74).padding(9)
        .neoWell(radius: MacTheme.Radius.compact)
    }
  }
}

extension String {
  fileprivate var nilIfEmpty: String? {
    let value = trimmingCharacters(in: .whitespacesAndNewlines)
    return value.isEmpty ? nil : value
  }
}

extension TaskState {
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

extension TaskPriority {
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

extension TaskReviewBadge {
  fileprivate var title: String { self == .your_turn ? "YOUR TURN" : "SYSTEM'S TURN" }
  fileprivate var color: Color { self == .your_turn ? MacTheme.accent : MacTheme.warning }
}

extension BlockingKind {
  fileprivate var title: String { rawValue.replacingOccurrences(of: "_", with: " ") }
}

extension AttemptResult {
  fileprivate var title: String { rawValue.capitalized }
}
