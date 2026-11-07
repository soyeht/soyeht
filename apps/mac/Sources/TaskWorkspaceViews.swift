import SoyehtClient
import SoyehtProtocol
import SwiftUI

enum TaskViewMode: String, CaseIterable {
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
    .paneShell()
    .padding(EdgeInsets(top: 8, leading: 6, bottom: 6, trailing: 6))
    .overlay(alignment: .top) {
      if showNewTask {
        ZStack(alignment: .top) {
          Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
            .onTapGesture {}
          NewTaskSheet(client: client, workspaceId: workspaceId, isPresented: $showNewTask)
            .padding(.top, DesignTokens.Layout.newTaskSheetTopInset).transition(
              .scale(scale: 0.98).combined(with: .opacity))
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
      Image(systemName: "checklist").font(
        .system(size: MacTheme.TypeScale.caption, weight: .semibold))
      Text("tasks · \(workspaceName)")
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .lineLimit(1).truncationMode(.tail).layoutPriority(2)
      Spacer(minLength: MacTheme.Space.x6)
      Button {
        showAttention.toggle()
      } label: {
        Text(summary).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
          .lineLimit(1).truncationMode(.tail)
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
    .font(.system(size: MacTheme.TypeScale.label, weight: .medium)).foregroundStyle(
      MacTheme.secondary
    )
    .padding(.horizontal, MacTheme.Space.x12).padding(.vertical, MacTheme.Space.x6)
    .background(MacTheme.well, in: Capsule())
  }

  /// The first density of `TaskToolbarDensity` that fits the width: labels give way to icons,
  /// then the chips go, so nothing wraps or overlaps.
  private var toolbar: some View {
    ViewThatFits(in: .horizontal) {
      ForEach(TaskToolbarDensity.allCases, id: \.self) { toolbar(density: $0) }
    }.frame(maxWidth: .infinity, minHeight: DesignTokens.Layout.taskToolbarHeight)
  }

  private func toolbar(density: TaskToolbarDensity) -> some View {
    HStack(spacing: MacTheme.Space.x12) {
      ForEach(density.items(listMode: mode == .list), id: \.self) { item in
        switch item {
        case .viewToggle:
          TaskViewToggle(mode: $mode, showsLabels: density.showsLabel(of: item))
          Spacer(minLength: 0)
        case .filter:
          TaskToolbarLabel(
            "Filter", icon: "line.3.horizontal.decrease", trailingIcon: "chevron.down",
            showsTitle: density.showsLabel(of: item)
          ).accessibilityHidden(true)
        case .priority:
          TaskToolbarLabel(
            "Priority", icon: "arrow.up.arrow.down", trailingIcon: "chevron.down",
            showsTitle: density.showsLabel(of: item)
          ).accessibilityHidden(true)
        case .group:
          TaskToolbarLabel(
            "Group: state", icon: "rectangle.3.group", trailingIcon: "chevron.down",
            showsTitle: density.showsLabel(of: item)
          ).accessibilityHidden(true)
        case .dispatch:
          Spacer(minLength: 0)
          dispatchMenu(showsTitle: density.showsLabel(of: item))
        case .newTask:
          Button {
            showNewTask = true
          } label: {
            TaskToolbarLabel(
              "New task", icon: "plus", trailingIcon: nil, accent: true,
              showsTitle: density.showsLabel(of: item))
          }.disabled(!client.connected).buttonStyle(.plain)
        }
      }
    }
  }

  private var emptyState: some View {
    ContentUnavailableView {
      Label(TaskPresentation.emptyTitle, systemImage: "checklist")
    } description: {
      Text(TaskPresentation.emptyDetail)
    } actions: {
      Button("New task") { showNewTask = true }.buttonStyle(NeoPillButtonStyle(accent: true))
    }.font(MacTheme.chrome(MacTheme.TypeScale.body))
      .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var summary: String {
    TaskPresentation.summary(tasks: tasks, attention: attention.count)
  }

  private var workspaceName: String {
    client.workspaces.first { $0.workspaceId == workspaceId }?.name ?? "Workspace"
  }

  private func dispatchMenu(showsTitle: Bool) -> some View {
    Menu {
      let ready = TaskPresentation.readyToDispatch(tasks)
      if ready.isEmpty {
        Text(TaskPresentation.noTaskReady)
      } else {
        ForEach(ready, id: \.taskId) { task in
          Button(TaskPresentation.dispatchLabel(task)) { client.dispatchTask(task) }
        }
      }
    } label: {
      TaskToolbarLabel(
        "Dispatch", icon: "paperplane", trailingIcon: "chevron.down", showsTitle: showsTitle)
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
          }.padding(MacTheme.Space.x12).neoWell(radius: MacTheme.Radius.compact)
        }
      }
    }.padding(MacTheme.Space.x20).attentionPopoverFrame().background(MacTheme.face).foregroundStyle(
      MacTheme.ink)
  }
}
