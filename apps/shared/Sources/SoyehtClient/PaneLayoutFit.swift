/// Panes shown one at a time behind a tab strip, like an adaptive writing companion in tabs.
public struct PaneLayoutTabGroup: Equatable, Sendable {
  /// The leaf that reserves the group's area in a presented tree. It names no pane.
  public static let slotPaneId = "soyeht.pane-layout.tab-group"

  /// Grouped panes in canonical order.
  public var paneIds: [String]
  /// The tab strip, in root content coordinates.
  public var stripRect: PaneLayoutRect
  /// The area of whichever grouped pane is shown, in root content coordinates.
  public var paneRect: PaneLayoutRect

  public init(paneIds: [String], stripRect: PaneLayoutRect, paneRect: PaneLayoutRect) {
    self.paneIds = paneIds
    self.stripRect = stripRect
    self.paneRect = paneRect
  }
}

public struct PaneLayoutFitResult: Equatable, Sendable {
  /// The tree that was laid out. Unless `keepsArrangement`, it is a presentation of the stored
  /// tree, and gestures that edit the stored tree do not apply to it.
  public var tree: PaneLayoutTree
  /// The floors used: compact floors replace full ones when the full floors do not fit.
  public var sizing: PaneLayoutSizingPolicy
  public var plan: PaneLayoutGeometryResult
  public var keepsArrangement: Bool
}

/// Presents a stored pane tree so that every pane keeps at least its floor and stays inside the
/// viewport. It never edits the stored tree; a larger viewport shows the stored arrangement again.
///
/// Like the writing companion, which goes side by side, then stacked, then tabs, it takes the
/// first presentation that fits:
/// 1. the stored arrangement;
/// 2. the stored arrangement at compact floors (the editor folds its project panel away);
/// 3. every pane in one row, then in one column;
/// 4. the primary panes in a row beside one tab group holding the auxiliary panes;
/// 5. one tab group holding every pane.
/// When nothing fits, as in a viewport smaller than a single floor, it uses the last
/// presentation and the geometry scrolls, as before.
public enum PaneLayoutFit {
  public static func layout(
    tree: PaneLayoutTree, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutFitResult {
    var compact = sizing
    compact.floors.merge(sizing.compactFloors) { _, compactFloor in compactFloor }
    var candidates = [
      Candidate(tree: tree, sizing: sizing), Candidate(tree: tree, sizing: compact),
    ]
    let units = tree.layoutUnits
    if units.count > 1 {
      for axis in [PaneLayoutAxis.horizontal, .vertical] {
        candidates.append(
          Candidate(tree: try compact.arranging(units, along: axis), sizing: compact))
      }
      let auxiliaries = units.filter { $0.groupableRole == .auxiliary }
      if auxiliaries.count > 1 {
        let others = units.filter { $0.groupableRole != .auxiliary }
        candidates.append(try compact.tabbing(auxiliaries, beside: others))
      }
      let groupable = units.filter { $0.groupableRole != nil }
      if groupable.count > 1 {
        let others = units.filter { $0.groupableRole == nil }
        candidates.append(try compact.tabbing(groupable, beside: others))
      }
    }
    let chosen =
      try candidates.first { try $0.fits(viewport) } ?? candidates[candidates.count - 1]
    var plan = try PaneLayoutGeometry.layout(
      tree: chosen.tree, viewport: viewport, sizing: chosen.sizing)
    if !chosen.tabbedPaneIds.isEmpty,
      let slot = plan.paneRects.removeValue(forKey: PaneLayoutTabGroup.slotPaneId)
    {
      let strip = PaneLayoutContentMetrics.companionTabStripHeight
      let paneRect = PaneLayoutRect(
        origin: PaneLayoutPoint(x: slot.origin.x, y: slot.origin.y + strip),
        size: PaneLayoutSize(width: slot.size.width, height: slot.size.height - strip))
      for paneId in chosen.tabbedPaneIds { plan.paneRects[paneId] = paneRect }
      plan.tabGroup = PaneLayoutTabGroup(
        paneIds: chosen.tabbedPaneIds,
        stripRect: PaneLayoutRect(
          origin: slot.origin, size: PaneLayoutSize(width: slot.size.width, height: strip)),
        paneRect: paneRect)
    }
    return PaneLayoutFitResult(
      tree: chosen.tree, sizing: chosen.sizing, plan: plan,
      keepsArrangement: chosen.tree == tree)
  }

  fileprivate struct Candidate {
    var tree: PaneLayoutTree
    var sizing: PaneLayoutSizingPolicy
    var tabbedPaneIds: [String] = []

    func fits(_ viewport: PaneLayoutSize) throws -> Bool {
      let minimum = try sizing.minimumSize(for: tree)
      return minimum.width <= viewport.width && minimum.height <= viewport.height
    }
  }
}

extension PaneLayoutTree {
  /// Leaves and writing companions, which move as one unit, in canonical order.
  fileprivate var layoutUnits: [PaneLayoutTree] {
    switch self {
    case .leaf:
      [self]
    case .split(let split):
      split.companion != nil ? [self] : split.children.flatMap { $0.tree.layoutUnits }
    }
  }

  /// The role of a unit that may join a tab group; a companion keeps its own presentation.
  fileprivate var groupableRole: PaneLayoutRole? {
    guard case .leaf(let leaf) = self else { return nil }
    return leaf.kind.role
  }
}

extension PaneLayoutSizingPolicy {
  fileprivate func arranging(
    _ units: [PaneLayoutTree], along axis: PaneLayoutAxis
  ) throws -> PaneLayoutTree {
    guard units.count > 1 else { return units[0] }
    return .split(PaneLayoutSplit(axis: axis, children: try branches(for: units, along: axis)))
  }

  /// A row of `others` beside one slot leaf whose floor holds the largest member and the strip.
  fileprivate func tabbing(
    _ members: [PaneLayoutTree], beside others: [PaneLayoutTree]
  ) throws -> PaneLayoutFit.Candidate {
    let floors = try members.map { try minimumSize(for: $0) }
    let groupFloor = PaneLayoutSize(
      width: floors.map(\.width).max() ?? 0,
      height: (floors.map(\.height).max() ?? 0)
        + PaneLayoutContentMetrics.companionTabStripHeight)
    let slot = PaneLayoutTree.leaf(
      PaneLayoutLeaf(paneId: PaneLayoutTabGroup.slotPaneId, kind: .agent))
    let sizing = overridingFloor(forPaneId: PaneLayoutTabGroup.slotPaneId, with: groupFloor)
    return PaneLayoutFit.Candidate(
      tree: try sizing.arranging(others + [slot], along: .horizontal), sizing: sizing,
      tabbedPaneIds: members.flatMap(\.paneIdsInCanonicalOrder))
  }
}
