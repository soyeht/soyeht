import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The paired Mac's workspaces and panes. Regular width keeps the sidebar next to the
/// selected pane; compact width (iPhone, narrow Split View) collapses to the list and pushes
/// the pane on top of it.
struct PhoneWorkspaceSplit<Status: View, Options: View>: View {
  let client: EngineClient
  /// A pane a tapped notification asked for; shown once the Mac state has it.
  @Binding var requestedPaneId: String?
  /// Shown in the sidebar while the Mac state is not synchronized.
  @ViewBuilder let status: () -> Status
  @ViewBuilder let options: () -> Options

  @Environment(\.horizontalSizeClass) private var sizeClass
  @State private var selection: String?
  @State private var columns = NavigationSplitViewVisibility.all
  @State private var action: PhoneWorkspaceAction?
  @FocusState private var sidebarFocused: Bool

  private var sections: [PaneSidebarSection] {
    PaneSidebarSection.sections(workspaces: client.workspaces, panes: client.panes)
  }
  private var fillsDetail: Bool { sizeClass == .regular }

  var body: some View {
    let sections = sections
    let order = PaneSidebarOrder(sections)
    NavigationSplitView(columnVisibility: $columns) {
      Group {
        if client.connected { sidebar(sections) } else { status() }
      }
      .phoneHomeColumn()
      .toolbar {
        ToolbarItemGroup(placement: .topBarTrailing) {
          if client.connected { PhoneAddMenu(action: $action) }
          options()
        }
      }
    } detail: {
      if let selection {
        detail(of: selection)
          .toolbar { closePaneButton(selection) }
      } else {
        ContentUnavailableView(
          "Choose a pane", systemImage: "sidebar.left",
          description: Text("Pick a pane in the sidebar to drive it here."))
      }
    }
    // Keep the pane beside the sidebar in portrait too, instead of dimmed under an overlay.
    .navigationSplitViewStyle(.balanced)
    .onChange(of: order, initial: true) { previous, current in
      reconcile(current, previous: previous)
      showRequestedPane()
    }
    .onChange(of: requestedPaneId) { showRequestedPane() }
    .onChange(of: fillsDetail) { reconcile(order, previous: order) }
    .focusedSceneValue(\.phonePaneNavigation, navigation(order, sections: sections))
    .phoneWorkspaceActions(client: client, action: $action, show: $requestedPaneId)
  }

  /// Closes the pane on screen. A document closes from its own menu.
  @ToolbarContentBuilder private func closePaneButton(_ paneId: String) -> some ToolbarContent {
    if client.panes.first(where: { $0.paneId == paneId })?.mode != .writing {
      ToolbarItem(placement: .topBarTrailing) {
        Button(role: .destructive) {
          action = .closePane(paneId)
        } label: {
          Label("Close Pane", systemImage: "xmark.circle")
        }
        .accessibilityIdentifier("close-pane")
      }
    }
  }

  /// A wide layout shows the selected pane's workspace as a board of panes side by side, with
  /// focus following the selection; the narrow layout shows the one pane.
  @ViewBuilder private func detail(of paneId: String) -> some View {
    if let pane = client.panes.first(where: { $0.paneId == paneId }),
      PaneBoardPolicy.usesBoard(
        regularWidth: fillsDetail,
        panes: client.layoutPanes(client.panes.filter { $0.workspaceId == pane.workspaceId }))
    {
      PhonePaneBoardView(client: client, workspaceId: pane.workspaceId, focusedPaneId: $selection)
        .id(pane.workspaceId)
    } else {
      PhonePaneDetail(client: client, paneId: paneId).id(paneId)
    }
  }

  private func sidebar(_ sections: [PaneSidebarSection]) -> some View {
    List(selection: $selection) {
      if client.workspaces.isEmpty {
        ContentUnavailableView {
          Label("No workspaces", systemImage: "rectangle.split.2x1")
        } description: {
          Text("Open a folder on the Mac to start here.")
        } actions: {
          Button("New Workspace") { action = .newWorkspace }
            .buttonStyle(.borderedProminent)
        }
        .listRowBackground(Color.clear)
      }
      ForEach(sections) { section in
        Section {
          ForEach(section.panes, id: \.paneId) { pane in
            NavigationLink(value: pane.paneId) { PhonePaneRow(pane: pane) }
              .listRowBackground(rowBackground(pane.paneId))
              .swipeActions(edge: .trailing) {
                Button(role: .destructive) {
                  action = .closePane(pane.paneId)
                } label: {
                  Label("Close", systemImage: "xmark")
                }
              }
              .contextMenu {
                Button(role: .destructive) {
                  action = .closePane(pane.paneId)
                } label: {
                  Label("Close Pane", systemImage: "xmark.circle")
                }
              }
          }
          if section.panes.isEmpty {
            Button("New pane in \(section.workspace.name)") {
              action = .newPane(workspaceId: section.workspace.workspaceId)
            }
            .listRowBackground(PhonePalette.face)
          }
        } header: {
          PhoneWorkspaceHeader(workspace: section.workspace, action: $action)
        }
      }
    }
    .scrollContentBackground(.hidden)
    .focused($sidebarFocused)
  }

  /// A custom row background hides the system highlight, so the selected row draws its own
  /// wherever the list and the pane are on screen together.
  private func rowBackground(_ paneId: String) -> Color {
    fillsDetail && selection == paneId ? PhonePalette.accent.opacity(0.18) : PhonePalette.face
  }

  private func showRequestedPane() {
    guard let pane = requestedPaneId, client.panes.contains(where: { $0.paneId == pane }) else {
      return
    }
    selection = pane
    requestedPaneId = nil
  }

  private func reconcile(_ order: PaneSidebarOrder, previous: PaneSidebarOrder) {
    selection = order.reconciled(
      selection, previous: previous, preferred: client.selectedPaneId, fillsDetail: fillsDetail)
  }

  private func navigation(_ order: PaneSidebarOrder, sections: [PaneSidebarSection])
    -> PhonePaneNavigation
  {
    PhonePaneNavigation(
      workspaceNames: sections.map(\.workspace.name),
      step: { offset in selection = order.step(from: selection, by: offset) },
      showWorkspace: { index in
        if let pane = order.pane(inWorkspaceAt: index, selection: selection) { selection = pane }
      },
      focusSidebar: {
        if fillsDetail { columns = .all } else { selection = nil }
        sidebarFocused = true
      })
  }
}

private struct PhonePaneRow: View {
  let pane: Pane

  var body: some View {
    Label {
      VStack(alignment: .leading, spacing: 5) {
        Text(pane.name).font(.system(.headline, design: .rounded))
        Text(pane.phrase).font(.caption).foregroundStyle(PhonePalette.secondary)
      }.padding(.vertical, 6)
    } icon: {
      Image(systemName: icon).foregroundStyle(PhonePalette.accent)
    }
  }

  private var icon: String {
    switch pane.mode {
    case .terminal: "terminal"
    case .agent: "bubble.left.and.bubble.right"
    case .editor: "doc.text"
    case .web: "globe"
    case .writing: "text.book.closed"
    }
  }
}
