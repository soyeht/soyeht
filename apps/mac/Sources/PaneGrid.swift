import AppKit
import OSLog
import SoyehtClient
import SoyehtProtocol
import SwiftUI

enum PaneSplitAxis: Sendable {
  case columns
  case rows
}

@MainActor struct NewPaneDraft: Equatable {
  let workspaceId: String?
  var insertion: PaneLayoutProvisionalInsertion

  var placeholderPaneId: String { insertion.placeholder.paneId }

  func previewTree() throws -> PaneLayoutTree {
    try insertion.previewTree(sizing: MacPaneLayoutMetrics.sizing)
  }

  func completedTree(
    paneId: String, kind: PaneLayoutKind, companionWritingPaneId: String?
  ) throws -> PaneLayoutTree {
    let leaf = PaneLayoutLeaf(paneId: paneId, kind: kind)
    if let companionWritingPaneId {
      return try insertion.completedWritingCompanionTree(
        with: leaf, writingPaneId: companionWritingPaneId,
        sizing: MacPaneLayoutMetrics.sizing)
    }
    return try insertion.completedTree(with: leaf, sizing: MacPaneLayoutMetrics.sizing)
  }

  mutating func select(kind: PaneLayoutKind) {
    insertion = insertion.selecting(kind: kind)
  }
}

struct NewPanePlaceholderPresentation {
  let draft: NewPaneDraft
  let onKindChange: (PaneLayoutKind) -> Void
  let onCancel: () -> Void
  let onCreated: (String, PaneLayoutKind) -> Void
}

struct PaneGrid: View {
  private static let logger = Logger(
    subsystem: "com.soyeht.next.dev.mac", category: "pane-layout")

  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String?
  let mode: PaneMode?
  let viewportSize: CGSize
  let layoutPersistenceEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onNewPane: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onTasks: () -> Void
  @Environment(EditorInputPreferences.self) private var editorPreferences
  @State private var interaction = PaneLayoutInteractionState()

  init(
    client: EngineClient, webHost: MacWebHost, workspaceId: String?, mode: PaneMode?,
    viewportSize: CGSize, layoutPersistenceEnabled: Bool = true,
    newPane: NewPanePlaceholderPresentation? = nil,
    onClose: @escaping (String) -> Void, onNewPane: @escaping () -> Void,
    onSplit: @escaping (String, PaneSplitAxis) -> Void, onTasks: @escaping () -> Void
  ) {
    self.client = client
    self.webHost = webHost
    self.workspaceId = workspaceId
    self.mode = mode
    self.viewportSize = viewportSize
    self.layoutPersistenceEnabled = layoutPersistenceEnabled
    self.newPane = newPane
    self.onClose = onClose
    self.onNewPane = onNewPane
    self.onSplit = onSplit
    self.onTasks = onTasks
  }

  private var workspacePanes: [Pane] {
    guard let workspaceId else { return [] }
    return client.panes.filter { $0.workspaceId == workspaceId }
  }

  private var panes: [Pane] {
    client.paneGridPanes(workspacePanes, mode: mode)
  }

  var body: some View {
    Group {
      if let layout = resolvedLayout {
        PaneLayoutTreeSurface(
          client: client, webHost: webHost, workspaceId: layout.workspaceId, panes: panes,
          tree: layout.tree, plan: layout.plan, viewportSize: viewportSize,
          interaction: interaction,
          interactionsEnabled: mode == nil && newPane == nil && layoutPersistenceEnabled
            && layout.keepsArrangement,
          newPane: newPane,
          onClose: onClose, onTasks: onTasks, onSplit: onSplit,
          onPaneDrop: commitPaneDrop
        )
        .task(id: layout.repairTaskID) {
          persistRepairIfNeeded(layout)
        }
      } else if panes.isEmpty, mode == .editor, let workspaceId {
        EditorEmptySurface(client: client, workspaceId: workspaceId, openFile: openEditorFile)
          .padding(PaneLayoutContentMetrics.paneCardInset)
      } else if panes.isEmpty {
        VStack(spacing: MacTheme.Space.x18) {
          SoyehtMark()
          Text("Your space to begin")
            .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
          Button("New pane", action: onNewPane).buttonStyle(NeoPillButtonStyle(accent: true))
        }
        .frame(width: viewportSize.width, height: viewportSize.height)
      }
    }
    .frame(width: viewportSize.width, height: viewportSize.height)
    .onChange(of: client.paneLayouts.failure) { _, failure in
      handleLayoutFailure(failure)
    }
    .onChange(of: activeDraftTree) { _, draft in
      if interaction.divider != nil, draft == nil { interaction.clear() }
    }
    .onChange(of: workspaceId) { previous, _ in
      if let previous { client.paneLayouts.cancelDraft(workspaceId: previous) }
      interaction.clear()
    }
    .onDisappear {
      if let workspaceId { client.paneLayouts.cancelDraft(workspaceId: workspaceId) }
      interaction.clear()
    }
    .onAppear { handleLayoutFailure(client.paneLayouts.failure) }
  }

  private func openEditorFile() {
    guard let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId }) else {
      return
    }
    let panel = NSOpenPanel()
    panel.directoryURL = URL(fileURLWithPath: workspace.path, isDirectory: true)
    panel.canChooseDirectories = false
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    guard panel.runModal() == .OK, let file = panel.url,
      let relative = relativePath(file, within: URL(fileURLWithPath: workspace.path))
    else { return }
    Task { @MainActor in
      do {
        _ = try await client.openEditorFile(workspaceId: workspace.workspaceId, path: relative)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func relativePath(_ file: URL, within root: URL) -> String? {
    let rootParts = root.standardizedFileURL.pathComponents
    let fileParts = file.standardizedFileURL.pathComponents
    guard fileParts.count > rootParts.count,
      Array(fileParts.prefix(rootParts.count)) == rootParts
    else { return nil }
    return fileParts.dropFirst(rootParts.count).joined(separator: "/")
  }

  private var projectPanel: EditorProjectPanelPreference {
    workspaceId.map { editorPreferences.projectPanel(for: $0) } ?? .automatic
  }

  private var resolvedLayout: ResolvedPaneGridLayout? {
    if let newPane, newPane.draft.workspaceId == workspaceId {
      do {
        let tree = try newPane.draft.previewTree()
        let viewport = PaneLayoutSize(width: viewportSize.width, height: viewportSize.height)
        let fit = try PaneLayoutFit.layout(
          tree: tree, viewport: viewport,
          sizing: MacPaneLayoutMetrics.sizing(
            newPanePlaceholderId: newPane.draft.placeholderPaneId), projectPanel: projectPanel)
        // The chooser has a presentation-only floor, so the fit never tabs it: when the stored
        // arrangement does not fit, the other panes share a tab group beside it.
        return ResolvedPaneGridLayout(
          workspaceId: workspaceId ?? "", state: PaneLayoutState(), tree: fit.tree,
          plan: fit.plan, insertedPaneIds: [], isProvisional: true, keepsArrangement: true)
      } catch {
        Self.logger.error(
          "new_pane_layout_resolution_failed error=\(error.localizedDescription)")
        return nil
      }
    }
    guard let workspaceId,
      let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return nil }
    do {
      let reconciliation = try client.paneLayouts.reconciledPresentation(
        for: workspace.workspaceId, livePanes: client.layoutPanes(workspacePanes),
        sizing: MacPaneLayoutMetrics.sizing)
      let state = reconciliation.state
      guard let completeTree = state.tree else { return nil }
      let visiblePaneIds = Set(panes.map(\.paneId))
      var visibleTree: PaneLayoutTree? = completeTree
      for paneId in completeTree.paneIdsInCanonicalOrder where !visiblePaneIds.contains(paneId) {
        visibleTree = visibleTree?.removingPane(withId: paneId)
      }
      guard let tree = visibleTree else { return nil }
      let fit = try PaneLayoutFit.layout(
        tree: tree,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.sizing, projectPanel: projectPanel)
      return ResolvedPaneGridLayout(
        workspaceId: workspaceId, state: state, tree: fit.tree, plan: fit.plan,
        insertedPaneIds: reconciliation.insertedPaneIds, isProvisional: false,
        keepsArrangement: fit.keepsArrangement)
    } catch {
      Self.logger.error("pane_layout_resolution_failed error=\(error.localizedDescription)")
      return nil
    }
  }

  private func persistRepairIfNeeded(_ layout: ResolvedPaneGridLayout) {
    guard layoutPersistenceEnabled, !layout.isProvisional, layout.state.mode == .custom,
      !layout.insertedPaneIds.isEmpty
    else { return }
    Self.logger.notice(
      "pane_layout_reconciled_missing workspace=\(layout.workspaceId, privacy: .public) panes=\(layout.insertedPaneIds.joined(separator: ","), privacy: .public)"
    )
    do {
      guard let tree = layout.state.tree else { return }
      try client.paneLayouts.commit(tree: tree, workspaceId: layout.workspaceId)
    } catch {
      client.problem = "Couldn't repair the pane layout. No panes were removed."
    }
  }

  private var activeDraftTree: PaneLayoutTree? {
    guard let workspaceId else { return nil }
    return client.paneLayouts.presentation(for: workspaceId)?.draftTree
  }

  private func commitPaneDrop(
    sourcePaneId: String, targetPaneId: String, zone: PaneLayoutDropZone
  ) {
    guard let workspaceId, sourcePaneId != targetPaneId else {
      interaction.clear()
      return
    }
    do {
      let tree = try client.paneLayouts.beginDraft(
        workspaceId: workspaceId, livePanes: client.layoutPanes(workspacePanes),
        sizing: MacPaneLayoutMetrics.sizing)
      let moved = try PaneLayoutTreeMutation.moving(
        tree, paneId: sourcePaneId, relativeTo: targetPaneId, zone: zone)
      try client.paneLayouts.updateDraft(workspaceId: workspaceId, tree: moved)
      try client.paneLayouts.commitDraft(workspaceId: workspaceId)
    } catch {
      client.paneLayouts.cancelDraft(workspaceId: workspaceId)
      client.problem = "Couldn't move the pane. The layout was not changed."
    }
    interaction.clear()
  }

  private func handleLayoutFailure(_ failure: PaneLayoutCommitFailure?) {
    guard let failure, failure.workspaceId == workspaceId else { return }
    client.paneLayouts.cancelDraft(workspaceId: failure.workspaceId)
    interaction.clear()
    client.problem = PaneLayoutCommitFailure.userMessage
    client.paneLayouts.clearFailure()
  }
}

private struct ResolvedPaneGridLayout {
  let workspaceId: String
  let state: PaneLayoutState
  let tree: PaneLayoutTree
  let plan: PaneLayoutGeometryResult
  let insertedPaneIds: [String]
  let isProvisional: Bool
  /// False while `PaneLayoutFit` rearranges the panes; layout gestures edit the stored tree.
  let keepsArrangement: Bool

  var repairTaskID: String {
    "\(workspaceId):\(state.mode.rawValue):\(isProvisional):\(insertedPaneIds.joined(separator: ","))"
  }
}

private struct PaneLayoutTreeSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String
  let panes: [Pane]
  let tree: PaneLayoutTree
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  var body: some View {
    PaneLayoutNodeSurface(
      client: client, webHost: webHost, panesById: panesById, tintIndices: tintIndices,
      workspaceId: workspaceId, tree: tree, path: PaneLayoutNodePath(), plan: plan,
      viewportSize: viewportSize, interaction: interaction,
      interactionsEnabled: interactionsEnabled, newPane: newPane,
      companionLinked: false, isVisible: true,
      onClose: onClose, onTasks: onTasks,
      onSplit: onSplit, onPaneDrop: onPaneDrop
    )
    .onDrop(
      of: PaneDragTransfer.acceptedTypes,
      delegate: PaneLayoutFallbackDropDelegate(interaction: interaction))
  }

  private var panesById: [String: Pane] {
    Dictionary(uniqueKeysWithValues: panes.map { ($0.paneId, $0) })
  }

  private var tintIndices: [String: Int] {
    Dictionary(
      uniqueKeysWithValues: tree.leavesInCanonicalOrder.enumerated().map {
        ($0.element.paneId, $0.offset)
      })
  }
}

struct PaneLayoutNodeSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panesById: [String: Pane]
  let tintIndices: [String: Int]
  let workspaceId: String
  let tree: PaneLayoutTree
  let path: PaneLayoutNodePath
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let companionLinked: Bool
  let isVisible: Bool
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  @ViewBuilder var body: some View {
    if let geometry = plan.nodes[path] {
      switch tree {
      case .leaf(let leaf):
        if let newPane, leaf.paneId == newPane.draft.placeholderPaneId {
          placeholderSurface(newPane, geometry: geometry)
        } else if let pane = panesById[leaf.paneId] {
          leafSurface(pane: pane, geometry: geometry)
        } else if let group = plan.tabGroup, leaf.paneId == PaneLayoutTabGroup.slotPaneId {
          tabGroupSurface(group, geometry: geometry)
        }
      case .split(let split):
        if let companion = split.companion {
          PaneLayoutCompanionSurface(
            client: client, webHost: webHost, panesById: panesById,
            tintIndices: tintIndices, workspaceId: workspaceId,
            split: split, companion: companion, path: path, geometry: geometry,
            viewportSize: viewportSize, interaction: interaction,
            interactionsEnabled: interactionsEnabled,
            onClose: onClose, onTasks: onTasks,
            onSplit: onSplit, onPaneDrop: onPaneDrop)
        } else {
          PaneLayoutSplitSurface(
            client: client, webHost: webHost, panesById: panesById, tintIndices: tintIndices,
            workspaceId: workspaceId, split: split, path: path, plan: plan,
            viewportSize: viewportSize, interaction: interaction,
            interactionsEnabled: interactionsEnabled, newPane: newPane,
            isVisible: isVisible,
            onClose: onClose, onTasks: onTasks,
            onSplit: onSplit, onPaneDrop: onPaneDrop)
        }
      }
    }
  }

  private func placeholderSurface(
    _ presentation: NewPanePlaceholderPresentation,
    geometry: PaneLayoutNodeGeometry
  ) -> some View {
    let halo = CGFloat(MacPaneLayoutMetrics.paneHalo)
    let width = CGFloat(geometry.frame.size.width)
    let height = CGFloat(geometry.frame.size.height)
    return NewPaneSheet(
      client: client, workspaceId: presentation.draft.workspaceId,
      mode: Binding(
        get: {
          presentation.draft.insertion.placeholder.kind == .agent ? .agent : .terminal
        },
        set: { presentation.onKindChange(PaneLayoutKind(mode: $0)) }),
      onCancel: presentation.onCancel, onCreated: presentation.onCreated
    )
    .frame(width: max(0, width - 2 * halo), height: max(0, height - 2 * halo))
    .padding(halo)
    .frame(width: width, height: height, alignment: .topLeading)
    .id(presentation.draft.placeholderPaneId)
    .accessibilityIdentifier("new-pane-placeholder")
    .onDrop(
      of: PaneDragTransfer.acceptedTypes,
      delegate: PaneLayoutChooserDropDelegate(interaction: interaction))
  }

  @ViewBuilder private func leafSurface(
    pane: Pane, geometry: PaneLayoutNodeGeometry
  ) -> some View {
    let halo = CGFloat(MacPaneLayoutMetrics.paneHalo)
    let contentWidth = CGFloat(geometry.contentSize.width)
    let contentHeight = CGFloat(geometry.contentSize.height)
    if geometry.overflowAxes.isEmpty {
      paneSurface(
        pane: pane, halo: halo, contentWidth: contentWidth, contentHeight: contentHeight
      )
      .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
      .id(pane.paneId)
    } else {
      ScrollView(Axis.Set(overflow: geometry.overflowAxes), showsIndicators: true) {
        paneSurface(
          pane: pane, halo: halo, contentWidth: contentWidth, contentHeight: contentHeight
        )
        .forceEnclosingOverlayScrollers()
      }
      .scrollBounceBehavior(.basedOnSize)
      .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
      .id(pane.paneId)
    }
  }

  private func paneSurface(
    pane: Pane, halo: CGFloat, contentWidth: CGFloat, contentHeight: CGFloat,
    visible: Bool = true
  ) -> some View {
    let cardWidth = max(0, contentWidth - 2 * halo)
    return PaneCard(
      client: client, webHost: webHost, pane: pane,
      tint: paneTint(tintIndices[pane.paneId] ?? 0), onClose: onClose,
      onTasks: onTasks, onSplit: onSplit,
      interaction: interactionsEnabled ? interaction : nil,
      companionLinked: companionLinked, isVisible: isVisible && visible, cardWidth: cardWidth,
      dropTargetSize: CGSize(
        width: cardWidth,
        height: max(0, contentHeight - 2 * halo)),
      onPaneDrop: onPaneDrop
    )
    .frame(
      width: cardWidth,
      height: max(0, contentHeight - 2 * halo)
    )
    .padding(halo)
    .frame(width: contentWidth, height: contentHeight, alignment: .topLeading)
  }

  /// Every grouped pane stays mounted; only the selected one, or the first, is visible. The
  /// strip stays put and the pane scrolls under it when the window is smaller than the pane's
  /// floor, so nothing is drawn past the slot.
  private func tabGroupSurface(
    _ group: PaneLayoutTabGroup, geometry: PaneLayoutNodeGeometry
  ) -> some View {
    let panes = group.paneIds.compactMap { panesById[$0] }
    let shown = panes.first { $0.paneId == client.selectedPaneId }?.paneId ?? group.paneIds.first
    let content = group.paneRect.size
    let slot = geometry.frame.size
    let area = group.paneArea(inSlot: slot)
    return VStack(spacing: 0) {
      PaneTabStrip(
        panes: panes, selectedPaneId: shown, identifier: "pane-group-tab",
        onSelect: { client.select($0) })
      PaneLayoutContainer(
        overflowAxes: Axis.Set(overflow: group.overflowAxes(inSlot: slot)),
        size: CGSize(width: area.width, height: area.height),
        contentSize: CGSize(width: content.width, height: content.height)
      ) {
        ZStack(alignment: .topLeading) {
          ForEach(panes, id: \.paneId) { pane in
            let visible = pane.paneId == shown
            paneSurface(
              pane: pane, halo: CGFloat(MacPaneLayoutMetrics.paneHalo),
              contentWidth: content.width, contentHeight: content.height, visible: visible
            )
            .id(pane.paneId)
            .opacity(visible ? 1 : 0)
            .allowsHitTesting(visible)
            .accessibilityHidden(!visible)
          }
        }
      }
    }
    .frame(width: slot.width, height: slot.height, alignment: .top)
  }

  private func paneTint(_ index: Int) -> Color {
    [MacTheme.agentRose, MacTheme.agentBlue, MacTheme.agentGreen, MacTheme.agentSand][index % 4]
  }
}
