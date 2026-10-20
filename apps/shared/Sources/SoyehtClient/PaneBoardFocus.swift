public enum PaneBoardDirection: Sendable {
  case left, right, up, down
}

/// Which pane has focus on a board, and where a hardware keyboard moves it. Focus follows a tap
/// on a pane; the keys step through the board in visual order or move to the pane beside.
extension PaneBoard {
  /// A region of the board that takes focus: a pane, or a tab group holding several.
  private struct Region {
    let paneIds: [String]
    let rect: PaneLayoutRect
  }

  private var regions: [Region] {
    placements.map { Region(paneIds: [$0.paneId], rect: $0.rect) }
      + (tabs.map { tabs in
        [
          Region(
            paneIds: tabs.paneIds,
            rect: PaneLayoutRect(
              origin: tabs.strip.origin,
              size: PaneLayoutSize(
                width: tabs.strip.size.width,
                height: tabs.strip.size.height + tabs.area.size.height)))
        ]
      } ?? [])
  }

  /// The focus after the board changed: the focused pane while it is on the board, else the pane
  /// `preferred` (the one focused elsewhere) when it is, else the first one.
  public func reconciledFocus(_ focused: String?, preferred: String?) -> String? {
    if let focused, paneIds.contains(focused) { return focused }
    if let preferred, paneIds.contains(preferred) { return preferred }
    return paneIds.first
  }

  /// The pane `offset` steps from `focused` in visual order, wrapping around the board.
  public func focus(after focused: String?, by offset: Int) -> String? {
    guard !paneIds.isEmpty else { return nil }
    guard let focused, let index = paneIds.firstIndex(of: focused) else {
      return offset >= 0 ? paneIds.first : paneIds.last
    }
    let count = paneIds.count
    return paneIds[((index + offset) % count + count) % count]
  }

  /// The tabbed pane to show: the focused one when it is in the group, else the one shown
  /// before, else the first.
  public func shownTab(focused: String?, remembered: String?) -> String? {
    guard let ids = tabs?.paneIds else { return nil }
    if let focused, ids.contains(focused) { return focused }
    if let remembered, ids.contains(remembered) { return remembered }
    return ids.first
  }

  /// The pane next to `focused` toward `direction`: among the regions that lie wholly on that
  /// side, the nearest one, the one sharing the most of the focused region's extent on a tie.
  /// A tab group answers with its `shown` pane. Nil when nothing lies that way.
  public func neighbor(
    of focused: String?, toward direction: PaneBoardDirection, shown: String?
  ) -> String? {
    guard let focused, let from = regions.first(where: { $0.paneIds.contains(focused) }) else {
      return nil
    }
    let origin = from.rect
    let beyond = regions.filter {
      !$0.paneIds.contains(focused) && Self.lies($0.rect, toward: direction, of: origin)
    }
    // A region that shares part of the extent is beside the pane; one that does not is a diagonal.
    let beside = beyond.filter { Self.score($0.rect, toward: direction, from: origin).overlap > 0 }
    let best = (beside.isEmpty ? beyond : beside).min { lhs, rhs in
      let left = Self.score(lhs.rect, toward: direction, from: origin)
      let right = Self.score(rhs.rect, toward: direction, from: origin)
      return left.gap != right.gap ? left.gap < right.gap : left.overlap > right.overlap
    }
    guard let best else { return nil }
    if let shown, best.paneIds.contains(shown) { return shown }
    return best.paneIds.first
  }

  private static func lies(
    _ rect: PaneLayoutRect, toward direction: PaneBoardDirection, of origin: PaneLayoutRect
  ) -> Bool {
    switch direction {
    case .left: rect.origin.x + rect.size.width <= origin.origin.x
    case .right: rect.origin.x >= origin.origin.x + origin.size.width
    case .up: rect.origin.y + rect.size.height <= origin.origin.y
    case .down: rect.origin.y >= origin.origin.y + origin.size.height
    }
  }

  private static func score(
    _ rect: PaneLayoutRect, toward direction: PaneBoardDirection, from origin: PaneLayoutRect
  ) -> (gap: Double, overlap: Double) {
    switch direction {
    case .left:
      (origin.origin.x - (rect.origin.x + rect.size.width), overlap(rect, origin, vertical: true))
    case .right:
      (rect.origin.x - (origin.origin.x + origin.size.width), overlap(rect, origin, vertical: true))
    case .up:
      (origin.origin.y - (rect.origin.y + rect.size.height), overlap(rect, origin, vertical: false))
    case .down:
      (
        rect.origin.y - (origin.origin.y + origin.size.height),
        overlap(rect, origin, vertical: false)
      )
    }
  }

  /// How much of the extent along the other axis two rectangles share.
  private static func overlap(_ a: PaneLayoutRect, _ b: PaneLayoutRect, vertical: Bool) -> Double {
    let (aStart, aLength) = vertical ? (a.origin.y, a.size.height) : (a.origin.x, a.size.width)
    let (bStart, bLength) = vertical ? (b.origin.y, b.size.height) : (b.origin.x, b.size.width)
    return max(0, min(aStart + aLength, bStart + bLength) - max(aStart, bStart))
  }
}
