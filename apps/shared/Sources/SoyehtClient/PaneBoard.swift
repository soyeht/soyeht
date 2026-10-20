import SoyehtProtocol

/// One pane drawn on the board, in the board's content coordinates.
public struct PaneBoardPlacement: Equatable, Sendable {
  public let paneId: String
  public let rect: PaneLayoutRect
}

/// The panes `PaneLayoutFit` folded into one tab group because they did not fit side by side.
public struct PaneBoardTabs: Equatable, Sendable {
  public let paneIds: [String]
  /// The tab strip.
  public let strip: PaneLayoutRect
  /// The area of whichever tabbed pane is shown, under the strip.
  public let area: PaneLayoutRect
}

/// A workspace's panes arranged for a wide device: the engine's stored layout fitted to the
/// viewport by `PaneLayoutFit`, the same rules the Mac and the desktop apply. It never edits the
/// stored layout; a larger viewport shows the stored arrangement again.
public struct PaneBoard: Equatable, Sendable {
  /// Panes drawn side by side, in visual order.
  public let placements: [PaneBoardPlacement]
  public let tabs: PaneBoardTabs?
  /// Every pane the board shows, in visual order; a tab group's panes take the place of its slot.
  public let paneIds: [String]
  /// The size the board occupies; larger than the viewport only when the viewport is smaller
  /// than the panes' floors, and then the board scrolls.
  public let contentSize: PaneLayoutSize
  /// False when `PaneLayoutFit` rearranged or tabbed the stored layout for this viewport.
  public let keepsArrangement: Bool

  public func overflowAxes(in viewport: PaneLayoutSize) -> Set<PaneLayoutAxis> {
    var axes: Set<PaneLayoutAxis> = []
    if contentSize.width > viewport.width { axes.insert(.horizontal) }
    if contentSize.height > viewport.height { axes.insert(.vertical) }
    return axes
  }
}

public enum PaneBoardPolicy {
  /// Whether a workspace is shown as a board rather than one pane at a time: a wide layout (a
  /// regular size class, not an iPhone or a narrow Split View) with more than one pane. The
  /// narrow layout keeps the list and the one-pane navigation. A workspace with a writing pane
  /// keeps the writing workspace, which arranges that pane and its companion itself.
  public static func usesBoard(regularWidth: Bool, panes: [Pane]) -> Bool {
    regularWidth && panes.count > 1 && !panes.contains { $0.mode == .writing }
  }

  /// The margin around the board, which holds the panes' shell shadows.
  public static let margin = PaneLayoutSpacing.outerMargin

  /// The size the board has inside a detail area of `area`.
  public static func viewport(in area: PaneLayoutSize) -> PaneLayoutSize {
    PaneLayoutSize(
      width: max(0, area.width - 2 * margin), height: max(0, area.height - 2 * margin))
  }

  /// `tree` without the panes `livePaneIds` does not hold, or nil when none is left.
  public static func visibleTree(_ tree: PaneLayoutTree, livePaneIds: Set<String>)
    -> PaneLayoutTree?
  {
    var visible: PaneLayoutTree? = tree
    for paneId in tree.paneIdsInCanonicalOrder where !livePaneIds.contains(paneId) {
      visible = visible?.removingPane(withId: paneId)
    }
    return visible
  }

  public static func board(
    tree: PaneLayoutTree, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy,
    projectPanel: EditorProjectPanelPreference = .automatic
  ) throws -> PaneBoard {
    let fit = try PaneLayoutFit.layout(
      tree: tree, viewport: viewport, sizing: sizing, projectPanel: projectPanel)
    let tabs = fit.plan.tabGroup.map {
      PaneBoardTabs(paneIds: $0.paneIds, strip: $0.stripRect, area: $0.paneRect)
    }
    let tabbed = Set(tabs?.paneIds ?? [])
    var paneIds: [String] = []
    for paneId in fit.tree.paneIdsInCanonicalOrder {
      if paneId == PaneLayoutTabGroup.slotPaneId {
        paneIds.append(contentsOf: tabs?.paneIds ?? [])
      } else {
        paneIds.append(paneId)
      }
    }
    let placements = paneIds.compactMap { paneId -> PaneBoardPlacement? in
      guard !tabbed.contains(paneId), let rect = fit.plan.paneRects[paneId] else { return nil }
      return PaneBoardPlacement(paneId: paneId, rect: rect)
    }
    let rects = placements.map(\.rect) + (tabs.map { [$0.strip, $0.area] } ?? [])
    return PaneBoard(
      placements: placements, tabs: tabs, paneIds: paneIds,
      contentSize: PaneLayoutSize(
        width: rects.map { $0.origin.x + $0.size.width }.max() ?? 0,
        height: rects.map { $0.origin.y + $0.size.height }.max() ?? 0),
      keepsArrangement: fit.keepsArrangement)
  }
}

extension EngineClient {
  /// The board of a workspace's panes in `viewport`, from the layout the engine stores for it
  /// (`workspace.layout.set`, so the Mac and this device agree). Nil when the workspace has no
  /// pane to show or the layout cannot be fitted, which is logged once.
  public func paneBoard(
    workspaceId: String, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) -> PaneBoard? {
    let live = layoutPanes(panes.filter { $0.workspaceId == workspaceId })
    guard
      let stored = presentedLayoutTree(workspaceId: workspaceId, livePanes: live, sizing: sizing),
      let tree = PaneBoardPolicy.visibleTree(stored, livePaneIds: Set(live.map(\.paneId)))
    else { return nil }
    return ClientDiagnostics.attempt("pane_board_failed", key: workspaceId) {
      try PaneBoardPolicy.board(tree: tree, viewport: viewport, sizing: sizing)
    }
  }
}
