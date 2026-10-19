import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// What the person asked to do to the Mac's workspaces and panes from the sidebar. Setting it
/// opens the sheet, question or name field the action needs; `phoneWorkspaceActions` carries
/// it out.
enum PhoneWorkspaceAction: Hashable, Identifiable {
  case newPane(workspaceId: String?)
  case newWorkspace
  case renameWorkspace(String)
  case closePane(String)
  case closeWorkspace(String)

  var id: Self { self }
}

extension View {
  func phoneWorkspaceActions(
    client: EngineClient, action: Binding<PhoneWorkspaceAction?>,
    show paneId: Binding<String?>
  ) -> some View {
    modifier(PhoneWorkspaceActions(client: client, action: action, requestedPaneId: paneId))
  }
}

private struct PhoneWorkspaceActions: ViewModifier {
  let client: EngineClient
  @Binding var action: PhoneWorkspaceAction?
  /// Set to the pane a new pane or workspace opened, so the split shows it once it arrives.
  @Binding var requestedPaneId: String?
  @State private var newName = ""

  func body(content: Content) -> some View {
    content
      .sheet(item: sheet) { item in
        switch item {
        case .newPane(let workspaceId):
          PhoneNewPaneSheet(client: client, workspaceId: workspaceId) { requestedPaneId = $0 }
        case .newWorkspace:
          PhoneNewWorkspaceSheet(client: client) { workspace in
            action = .newPane(workspaceId: workspace.workspaceId)
          }
        default:
          EmptyView()
        }
      }
      .alert(
        confirmation?.title ?? "", isPresented: asking, presenting: confirmation
      ) { confirmation in
        Button("Cancel", role: .cancel) { action = nil }
        Button(confirmation.confirmTitle, role: .destructive) { carryOut(action) }
      } message: { confirmation in
        Text(confirmation.message)
      }
      .alert("Rename workspace", isPresented: renaming) {
        TextField("Name", text: $newName)
        Button("Rename", action: rename)
        Button("Cancel", role: .cancel) { action = nil }
      }
      .alert("Could not complete", isPresented: failing) {
        Button("OK") { client.problem = nil }
      } message: {
        Text(client.problem ?? "")
      }
      .onChange(of: action) { _, next in
        if let next, confirmation == nil { carryOutAtOnce(next) }
        if case .renameWorkspace(let id) = next {
          newName = client.workspaces.first { $0.workspaceId == id }?.name ?? ""
        }
      }
  }

  /// The sheet the action opens. Closing the sheet clears the action.
  private var sheet: Binding<PhoneWorkspaceAction?> {
    Binding(
      get: {
        switch action {
        case .newPane, .newWorkspace: action
        default: nil
        }
      },
      set: { if $0 == nil { clearIfSheet() } })
  }

  private func clearIfSheet() {
    switch action {
    case .newPane, .newWorkspace: action = nil
    default: break
    }
  }

  /// The question the close action asks first, or nil when it closes at once.
  private var confirmation: CloseConfirmation? {
    switch action {
    case .closePane(let id): client.closeConfirmation(forPane: id)
    case .closeWorkspace(let id): client.closeConfirmation(forWorkspace: id)
    default: nil
    }
  }

  private var asking: Binding<Bool> {
    // The alert's own buttons end the action; dismissing it must not clear it first.
    Binding(get: { confirmation != nil }, set: { _ in })
  }

  private var renaming: Binding<Bool> {
    Binding(
      get: {
        if case .renameWorkspace = action { return true }
        return false
      },
      set: { _ in })
  }

  private var failing: Binding<Bool> {
    Binding(get: { client.problem != nil }, set: { if !$0 { client.problem = nil } })
  }

  private func carryOutAtOnce(_ next: PhoneWorkspaceAction) {
    switch next {
    case .closePane, .closeWorkspace: carryOut(next)
    default: break
    }
  }

  private func carryOut(_ next: PhoneWorkspaceAction?) {
    action = nil
    switch next {
    case .closePane(let id):
      Task { await client.closePaneSavingEdits(id) }
    case .closeWorkspace(let id):
      Task { await client.closeWorkspaceReportingFailure(id) }
    default:
      break
    }
  }

  private func rename() {
    defer { action = nil }
    guard case .renameWorkspace(let id) = action,
      let current = client.workspaces.first(where: { $0.workspaceId == id })?.name,
      let name = WorkspaceNaming.submission(newName, current: current)
    else { return }
    Task {
      do {
        try await client.renameWorkspace(id, to: name)
      } catch {
        client.problem = "Couldn't rename the workspace."
      }
    }
  }
}

/// The "+" of the sidebar: a pane in the workspace on screen, or a workspace for a new folder.
struct PhoneAddMenu: View {
  @Binding var action: PhoneWorkspaceAction?

  var body: some View {
    Menu {
      Button {
        action = .newPane(workspaceId: nil)
      } label: {
        Label("New Pane", systemImage: "plus.rectangle.on.rectangle")
      }
      .accessibilityIdentifier("sidebar-new-pane")
      Button {
        action = .newWorkspace
      } label: {
        Label("New Workspace", systemImage: "folder.badge.plus")
      }
      .accessibilityIdentifier("sidebar-new-workspace")
    } label: {
      Image(systemName: "plus")
    }
    .accessibilityLabel("New")
    .accessibilityIdentifier("sidebar-add")
  }
}

/// A workspace's name in the sidebar, with the actions that belong to it.
struct PhoneWorkspaceHeader: View {
  let workspace: Workspace
  @Binding var action: PhoneWorkspaceAction?

  var body: some View {
    HStack {
      Text(workspace.name)
      Spacer()
      Menu {
        Button {
          action = .newPane(workspaceId: workspace.workspaceId)
        } label: {
          Label("New Pane Here", systemImage: "plus.rectangle.on.rectangle")
        }
        Button {
          action = .renameWorkspace(workspace.workspaceId)
        } label: {
          Label("Rename", systemImage: "pencil")
        }
        Button(role: .destructive) {
          action = .closeWorkspace(workspace.workspaceId)
        } label: {
          Label("Close Workspace", systemImage: "xmark.circle")
        }
      } label: {
        Image(systemName: "ellipsis.circle")
      }
      .accessibilityLabel("\(workspace.name) actions")
      .accessibilityIdentifier("workspace-actions-\(workspace.workspaceId)")
    }
  }
}
