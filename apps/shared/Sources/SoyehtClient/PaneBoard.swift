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

  /// Where `paneId` is drawn: its own cell, or the area of the tab group when it is a tab.
  public func rect(of paneId: String) -> PaneLayoutRect? {
    if let placement = placements.first(where: { $0.paneId == paneId }) { return placement.rect }
    return tabs.flatMap { $0.paneIds.contains(paneId) ? $0.area : nil }
  }

  /// The top of what `paneId` is shown with: its tab strip when it is a tab.
  public func topEdge(of paneId: String) -> Double? {
    if let tabs, tabs.paneIds.contains(paneId) { return tabs.strip.origin.y }
    return rect(of: paneId)?.origin.y
  }

  /// This board with `paneId` drawn `height` tall.
  public func resizingPane(_ paneId: String, toHeight height: Double) -> PaneBoard {
    var board = self
    var placements = self.placements
    if let index = placements.firstIndex(where: { $0.paneId == paneId }) {
      var rect = placements[index].rect
      rect.size.height = height
      placements[index] = PaneBoardPlacement(paneId: paneId, rect: rect)
      board = PaneBoard(
        placements: placements, tabs: tabs, paneIds: paneIds, contentSize: contentSize,
        keepsArrangement: keepsArrangement)
    } else if let tabs, tabs.paneIds.contains(paneId) {
      var area = tabs.area
      area.size.height = height
      board = PaneBoard(
        placements: placements,
        tabs: PaneBoardTabs(paneIds: tabs.paneIds, strip: tabs.strip, area: area),
        paneIds: paneIds, contentSize: contentSize, keepsArrangement: keepsArrangement)
    }
    return board
  }

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

  /// How much of the area the on-screen keyboard covers, from the area's height without the
  /// keyboard and the height that is left above it. The board is laid out in the first, so that
  /// the keyboard never changes how the panes are arranged (side by side, stacked or tabbed).
  public static func keyboardInset(areaHeight: Double, visibleHeight: Double) -> Double {
    max(0, areaHeight - visibleHeight)
  }

  /// Where the focused pane goes while the keyboard is up: the board moves up by `shift`, and
  /// the pane is cut down to `height` when it is taller than what is left above the keyboard.
  public struct KeyboardFit: Equatable, Sendable {
    public let shift: Double
    /// The focused pane's height above the keyboard, or nil when it keeps its own.
    public let height: Double?
    public static let none = KeyboardFit(shift: 0, height: nil)
  }

  /// Keeps the bottom of the focused pane, where the prompt or the cursor is, a margin above the
  /// keyboard without touching the arrangement: a pane that fits above the keyboard moves up
  /// just far enough, and a taller one is cut down to the room above the keyboard with its top
  /// (and the tab strip over it) in view. Nothing moves while the pane ends above the keyboard.
  public static func keyboardFit(
    board: PaneBoard, focused: String?, visibleHeight: Double, inset: Double
  ) -> KeyboardFit {
    guard inset > 0, let focused, let rect = board.rect(of: focused) else { return .none }
    // The board sits one margin inside the area, and leaves one margin below the pane.
    let room = visibleHeight - 2 * margin
    let bottom = rect.origin.y + rect.size.height
    guard bottom > room else { return .none }
    let top = board.topEdge(of: focused) ?? rect.origin.y
    if bottom - top <= room { return KeyboardFit(shift: bottom - room, height: nil) }
    return KeyboardFit(shift: top, height: max(0, room - (rect.origin.y - top)))
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
