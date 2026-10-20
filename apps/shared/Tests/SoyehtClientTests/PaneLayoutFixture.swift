import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The sizing the pane layout tests share: the accepted font metrics, the footprint floors with
/// their compact floors, and the automatic arrangement the Mac builds as panes open.
enum PaneLayoutFixture {
  static let fonts = acceptedFontMetrics
  static let halo = PaneLayoutSpacing.paneHalo
  static let floors = PaneLayoutFloorFormula.footprintFloors(fonts: fonts, halo: halo)
  static let compactFloors = PaneLayoutFloorFormula.compactFootprintFloors(
    fonts: fonts, halo: halo)
  static let sizing = PaneLayoutSizingPolicy(
    floors: floors, gaps: PaneLayoutSpacing.gaps, compactFloors: compactFloors)

  /// A live idle pane of `mode` in `workspace`.
  static func pane(_ id: String, workspace: String, mode: PaneMode) -> Pane {
    Pane(
      paneId: id, workspaceId: workspace, name: id, cwd: "/repo", mode: mode, state: .idle,
      source: .protocol, confidence: 1, phrase: "idle", generation: 1, cols: 80, rows: 24,
      record: false)
  }

  /// The Mac's window policy around a pane footprint of at most `largestWidth` and at least
  /// `smallestHeight`.
  static func window(largestWidth: Double, smallestHeight: Double = 0) -> PaneLayoutWindowPolicy {
    PaneLayoutWindowPolicy(
      minimumExercisedViewport: PaneLayoutSize(width: 375, height: 375),
      largestPaneFootprintWidth: largestWidth, smallestPaneFootprintHeight: smallestHeight,
      standardHorizontalInset: PaneLayoutSpacing.outerMargin, topInset: 8, topBarHeight: 42,
      paneVerticalInset: PaneLayoutSpacing.outerMargin, railWidth: 56)
  }

  /// The tree of panes of `kinds`, inserted one after the other, editor first.
  static func workspace(_ kinds: [PaneLayoutKind]) throws -> PaneLayoutTree {
    var tree: PaneLayoutTree?
    for (index, kind) in kinds.enumerated() {
      tree = try PaneLayoutArrangement.inserting(
        PaneLayoutLeaf(paneId: "\(kind)-\(index)", kind: kind), into: tree,
        rule: .initialByType, sizing: sizing)
    }
    return try #require(tree)
  }
}
