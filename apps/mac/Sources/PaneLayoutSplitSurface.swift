import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PaneLayoutCompanionSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panesById: [String: Pane]
  let tintIndices: [String: Int]
  let workspaceId: String
  let split: PaneLayoutSplit
  let companion: PaneLayoutCompanion
  let path: PaneLayoutNodePath
  let geometry: PaneLayoutNodeGeometry
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  var body: some View {
    if let layout = layoutPlan {
      Group {
        if layout.overflowAxes.isEmpty {
          companionContent(layout)
        } else {
          ScrollView(Axis.Set(overflow: layout.overflowAxes), showsIndicators: true) {
            companionContent(layout)
              .forceEnclosingOverlayScrollers()
          }
          .scrollBounceBehavior(.basedOnSize)
        }
      }
      .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
      .onChange(of: layout.presentation) { previous, next in
        if previous != next { dividers.cancelIfActive() }
      }
    }
  }

  private var layoutPlan: WritingCompanionLayoutPlan? {
    guard let writingPane = panesById[companion.writingPaneId] else { return nil }
    let writingKind = PaneLayoutKind(
      mode: writingPane.mode, writingKind: writingPane.writingKind)
    let writingFont: PaneLayoutFontMetric
    if writingKind == .screenplay {
      writingFont = MacPaneLayoutMetrics.screenplayMetric
    } else {
      let document = client.writingDocuments.values.first {
        $0.document.file.paneId == writingPane.paneId
      }
      writingFont = MacPaneLayoutMetrics.proseMetric(for: document?.document.font ?? .reading_serif)
    }
    let policy = WritingLayoutPolicy(
      floors: MacPaneLayoutMetrics.footprintFloors,
      gaps: MacPaneLayoutMetrics.sizing.gaps)
    return try? policy.companionLayout(
      WritingCompanionRequest(
        companion: companion, canonicalPaneIds: directLeaves.map(\.paneId),
        writingKind: writingKind, writingFont: writingFont,
        viewport: geometry.contentSize, selectedPaneId: client.selectedPaneId))
  }

  private var directLeaves: [PaneLayoutLeaf] {
    split.children.compactMap { branch in
      guard case .leaf(let leaf) = branch.tree else { return nil }
      return leaf
    }
  }

  @ViewBuilder private func companionContent(_ layout: WritingCompanionLayoutPlan) -> some View {
    let pairGeometry = presentedGeometry(for: layout)
    ZStack(alignment: .topLeading) {
      Color.clear
        .frame(width: layout.contentSize.width, height: layout.contentSize.height)
        .allowsHitTesting(false)

      ForEach(Array(directLeaves.enumerated()), id: \.element.paneId) { index, leaf in
        if let rect = companionRect(
          at: index, layout: layout, pairGeometry: pairGeometry)
        {
          companionLeaf(
            leaf, rect: rect,
            visible: layout.presentation != .tabs || leaf.paneId == layout.selectedPaneId)
        }
      }

      if layout.presentation == .tabs, let tabStrip = layout.tabStripRect {
        companionTabs(layout)
          .writingFrame(tabStrip)
          .zIndex(20)
      }

      if interactionsEnabled, let axis = layout.presentation.splitAxis,
        let pairGeometry,
        let divider = dividerFrame(
          axis: axis, geometry: pairGeometry, contentOrigin: layout.contentRect.origin)
      {
        PaneLayoutDividerHandle(
          axis: axis, accessibilityID: dividerAccessibilityID,
          onChanged: { translation in
            resizeDivider(
              axis: axis, translation: translation, viewport: layout.contentRect.size)
          },
          onEnded: { dividers.finish(afterChildIndex: 0) },
          onDoubleClick: {
            equalize(axis: axis, viewport: layout.contentRect.size)
          }
        )
        .frame(width: divider.size.width, height: divider.size.height)
        .offset(x: divider.origin.x, y: divider.origin.y)
        .zIndex(50)
      }
    }
    .frame(
      width: layout.contentSize.width, height: layout.contentSize.height,
      alignment: .topLeading)
  }

  private func companionTabs(_ layout: WritingCompanionLayoutPlan) -> some View {
    PaneTabStrip(
      panes: layout.tabOrder.compactMap { panesById[$0] }, selectedPaneId: layout.selectedPaneId,
      identifier: "writing-companion-tab", onSelect: { client.select($0) })
  }

  @ViewBuilder private func companionLeaf(
    _ leaf: PaneLayoutLeaf, rect: PaneLayoutRect, visible: Bool
  ) -> some View {
    if panesById[leaf.paneId] != nil,
      let leafPlan = try? PaneLayoutGeometry.layout(
        tree: .leaf(leaf), viewport: rect.size, sizing: MacPaneLayoutMetrics.sizing)
    {
      PaneLayoutNodeSurface(
        client: client, webHost: webHost, panesById: panesById,
        tintIndices: tintIndices, workspaceId: workspaceId,
        tree: .leaf(leaf), path: PaneLayoutNodePath(), plan: leafPlan,
        viewportSize: CGSize(width: rect.size.width, height: rect.size.height),
        interaction: interaction, interactionsEnabled: interactionsEnabled,
        newPane: nil, companionLinked: true, isVisible: visible,
        onClose: onClose, onTasks: onTasks,
        onSplit: onSplit, onPaneDrop: onPaneDrop
      )
      .frame(width: rect.size.width, height: rect.size.height)
      .offset(x: rect.origin.x, y: rect.origin.y)
      .opacity(visible ? 1 : 0)
      .allowsHitTesting(visible)
      .accessibilityHidden(!visible)
      .zIndex(visible ? 10 : 0)
    }
  }

  private func presentedGeometry(
    for layout: WritingCompanionLayoutPlan
  ) -> PaneLayoutGeometryResult? {
    guard let axis = layout.presentation.splitAxis else { return nil }
    let presented = PaneLayoutTree.split(
      PaneLayoutSplit(axis: axis, children: split.children))
    return try? PaneLayoutGeometry.layout(
      tree: presented, viewport: layout.contentRect.size,
      sizing: MacPaneLayoutMetrics.sizing)
  }

  private func companionRect(
    at index: Int, layout: WritingCompanionLayoutPlan,
    pairGeometry: PaneLayoutGeometryResult?
  ) -> PaneLayoutRect? {
    if layout.presentation == .tabs { return layout.contentRect }
    guard let child = pairGeometry?.nodes[PaneLayoutNodePath(components: [index])] else {
      return nil
    }
    return translated(child.frame, by: layout.contentRect.origin)
  }

  private func dividerFrame(
    axis: PaneLayoutAxis, geometry: PaneLayoutGeometryResult,
    contentOrigin: PaneLayoutPoint
  ) -> PaneLayoutRect? {
    guard let first = geometry.nodes[PaneLayoutNodePath(components: [0])],
      let root = geometry.nodes[PaneLayoutNodePath()]
    else { return nil }
    if axis == .horizontal {
      return PaneLayoutRect(
        origin: PaneLayoutPoint(
          x: contentOrigin.x + first.frame.origin.x + first.frame.size.width,
          y: contentOrigin.y),
        size: PaneLayoutSize(
          width: MacPaneLayoutMetrics.sizing.gaps.horizontal,
          height: root.contentSize.height))
    }
    return PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: contentOrigin.x,
        y: contentOrigin.y + first.frame.origin.y + first.frame.size.height),
      size: PaneLayoutSize(
        width: root.contentSize.width,
        height: MacPaneLayoutMetrics.sizing.gaps.vertical))
  }

  private func translated(
    _ rect: PaneLayoutRect, by origin: PaneLayoutPoint
  ) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: rect.origin.x + origin.x, y: rect.origin.y + origin.y),
      size: rect.size)
  }

  private var dividerAccessibilityID: String {
    let value =
      path.components.isEmpty ? "root" : path.components.map(String.init).joined(separator: "-")
    return "pane-divider-\(value)-0"
  }

  private var dividers: PaneDividerEditing {
    PaneDividerEditing(
      client: client, workspaceId: workspaceId, interaction: interaction, splitPath: path)
  }

  private func resizeDivider(
    axis: PaneLayoutAxis, translation: CGSize, viewport: PaneLayoutSize
  ) {
    let delta = axis == .horizontal ? translation.width : translation.height
    dividers.resize(afterChildIndex: 0) { base in
      try PaneLayoutTreeMutation.resizingPresentedCompanion(
        base, splitAt: path, presentationAxis: axis,
        dividerIndex: 0, delta: delta, viewport: viewport,
        sizing: MacPaneLayoutMetrics.sizing)
    }
  }

  private func equalize(axis: PaneLayoutAxis, viewport: PaneLayoutSize) {
    dividers.equalize { tree in
      try PaneLayoutTreeMutation.equalizingPresentedCompanion(
        tree, splitAt: path, presentationAxis: axis,
        viewport: viewport, sizing: MacPaneLayoutMetrics.sizing)
    }
  }
}

struct PaneLayoutSplitSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panesById: [String: Pane]
  let tintIndices: [String: Int]
  let workspaceId: String
  let split: PaneLayoutSplit
  let path: PaneLayoutNodePath
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let isVisible: Bool
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void
  @Environment(EditorInputPreferences.self) private var editorPreferences

  private var projectPanel: EditorProjectPanelPreference {
    editorPreferences.projectPanel(for: workspaceId)
  }

  private var children: [RenderedPaneLayoutChild] {
    split.children.enumerated().map {
      RenderedPaneLayoutChild(index: $0.offset, branch: $0.element)
    }
  }

  var body: some View {
    if let geometry = plan.nodes[path] {
      ScrollViewReader { proxy in
        PaneLayoutContainer(
          overflowAxes: geometry.overflowAxes.contains(split.axis)
            ? (split.axis == .horizontal ? .horizontal : .vertical) : [],
          size: CGSize(width: geometry.frame.size.width, height: geometry.frame.size.height),
          contentSize: CGSize(
            width: geometry.contentSize.width, height: geometry.contentSize.height)
        ) {
          ZStack(alignment: .topLeading) {
            ForEach(children) { child in
              let childPath = path.appending(child.index)
              if let childGeometry = plan.nodes[childPath] {
                PaneLayoutNodeSurface(
                  client: client, webHost: webHost, panesById: panesById,
                  tintIndices: tintIndices, workspaceId: workspaceId,
                  tree: child.branch.tree, path: childPath, plan: plan,
                  viewportSize: viewportSize, interaction: interaction,
                  interactionsEnabled: interactionsEnabled, newPane: newPane,
                  companionLinked: false, isVisible: isVisible,
                  onClose: onClose,
                  onTasks: onTasks, onSplit: onSplit, onPaneDrop: onPaneDrop
                )
                .frame(
                  width: childGeometry.frame.size.width,
                  height: childGeometry.frame.size.height
                )
                .offset(
                  x: childGeometry.frame.origin.x,
                  y: childGeometry.frame.origin.y
                )
                .id(child.id)
              }
            }
            if interactionsEnabled {
              ForEach(dividers) { divider in
                if let frame = dividerFrame(divider, in: geometry) {
                  PaneLayoutDividerHandle(
                    axis: divider.axis,
                    accessibilityID: divider.accessibilityID,
                    onChanged: { translation in
                      resizeDivider(divider, translation: translation)
                    },
                    onEnded: { dividerEditing.finish(afterChildIndex: divider.afterChildIndex) },
                    onDoubleClick: { equalize() }
                  )
                  .frame(width: frame.width, height: frame.height)
                  .offset(x: frame.minX, y: frame.minY)
                  .zIndex(50)
                }
              }
            }
          }
        }
        .onAppear { scrollSelectedPane(using: proxy) }
        .onChange(of: client.selectedPaneId) { _, _ in scrollSelectedPane(using: proxy) }
      }
    }
  }

  private func scrollSelectedPane(using proxy: ScrollViewProxy) {
    guard let selectedPaneId = client.selectedPaneId,
      let childIndex = split.children.firstIndex(where: {
        $0.tree.paneIdsInCanonicalOrder.contains(selectedPaneId)
      })
    else { return }
    let identity = split.children[childIndex].tree.leavesInCanonicalOrder[0].paneId
    proxy.scrollTo(identity, anchor: .center)
  }

  private var dividers: [RenderedPaneLayoutDivider] {
    plan.dividers.compactMap { divider in
      guard divider.splitPath == path else { return nil }
      return RenderedPaneLayoutDivider(geometry: divider)
    }
  }

  private func dividerFrame(
    _ divider: RenderedPaneLayoutDivider, in geometry: PaneLayoutNodeGeometry
  ) -> CGRect? {
    guard let before = plan.nodes[path.appending(divider.afterChildIndex)] else { return nil }
    if divider.axis == .horizontal {
      return CGRect(
        x: before.frame.maxX, y: 0,
        width: MacPaneLayoutMetrics.sizing.gaps.horizontal,
        height: geometry.contentSize.height)
    }
    return CGRect(
      x: 0, y: before.frame.maxY,
      width: geometry.contentSize.width,
      height: MacPaneLayoutMetrics.sizing.gaps.vertical)
  }

  private var dividerEditing: PaneDividerEditing {
    PaneDividerEditing(
      client: client, workspaceId: workspaceId, interaction: interaction, splitPath: path)
  }

  private func resizeDivider(
    _ divider: RenderedPaneLayoutDivider, translation: CGSize
  ) {
    let delta = divider.axis == .horizontal ? translation.width : translation.height
    dividerEditing.resize(afterChildIndex: divider.afterChildIndex) { base in
      try PaneLayoutTreeMutation.resizing(
        base, splitAt: path, dividerIndex: divider.afterChildIndex, delta: delta,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.fittedSizing(
          base, viewport: viewportSize, projectPanel: projectPanel))
    }
  }

  private func equalize() {
    dividerEditing.equalize { tree in
      try PaneLayoutTreeMutation.equalizing(
        tree, splitAt: path,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.fittedSizing(
          tree, viewport: viewportSize, projectPanel: projectPanel))
    }
  }
}

private struct RenderedPaneLayoutChild: Identifiable {
  let index: Int
  let branch: PaneLayoutBranch

  var id: String { branch.tree.leavesInCanonicalOrder[0].paneId }
}

private struct RenderedPaneLayoutDivider: Identifiable {
  let splitPath: PaneLayoutNodePath
  let afterChildIndex: Int
  let axis: PaneLayoutAxis

  init(geometry: PaneLayoutDividerGeometry) {
    splitPath = geometry.splitPath
    afterChildIndex = geometry.afterChildIndex
    axis = geometry.axis
  }

  var id: String { accessibilityID }
  var accessibilityID: String {
    let path =
      splitPath.components.isEmpty
      ? "root" : splitPath.components.map(String.init).joined(separator: "-")
    return "pane-divider-\(path)-\(afterChildIndex)"
  }
}
