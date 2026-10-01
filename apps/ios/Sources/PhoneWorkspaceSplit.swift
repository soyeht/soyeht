import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The paired Mac's workspaces and panes. Regular width keeps the sidebar next to the
/// selected pane; compact width (iPhone, narrow Split View) collapses to the list and pushes
/// the pane on top of it.
struct PhoneWorkspaceSplit<Status: View, Options: View>: View {
  let client: EngineClient
  /// Shown in the sidebar while the Mac state is not synchronized.
  @ViewBuilder let status: () -> Status
  @ViewBuilder let options: () -> Options

  @Environment(\.horizontalSizeClass) private var sizeClass
  @State private var selection: String?
  @State private var columns = NavigationSplitViewVisibility.all
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
      .toolbar { ToolbarItem(placement: .topBarTrailing) { options() } }
    } detail: {
      if let selection {
        PhonePaneDetail(client: client, paneId: selection).id(selection)
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
    }
    .onChange(of: fillsDetail) { reconcile(order, previous: order) }
    .focusedSceneValue(\.phonePaneNavigation, navigation(order, sections: sections))
  }

  private func sidebar(_ sections: [PaneSidebarSection]) -> some View {
    List(selection: $selection) {
      if client.panes.isEmpty {
        ContentUnavailableView(
          "No open panes", systemImage: "rectangle.split.2x1",
          description: Text("Open a pane on the Mac to continue here.")
        )
        .listRowBackground(Color.clear)
      }
      ForEach(sections) { section in
        Section(section.workspace.name) {
          ForEach(section.panes, id: \.paneId) { pane in
            NavigationLink(value: pane.paneId) { PhonePaneRow(pane: pane) }
              .listRowBackground(rowBackground(pane.paneId))
          }
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
