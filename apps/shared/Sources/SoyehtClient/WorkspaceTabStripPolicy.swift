public struct WorkspaceTabStripItem: Equatable, Sendable {
  public var workspaceId: String
  public var minX: Double
  public var width: Double

  public init(workspaceId: String, minX: Double, width: Double) {
    self.workspaceId = workspaceId
    self.minX = minX
    self.width = width
  }

  public var maxX: Double { minX + width }
}

public struct WorkspaceTabStripViewport: Equatable, Sendable {
  public var offset: Double
  public var width: Double

  public init(offset: Double, width: Double) {
    self.offset = offset
    self.width = width
  }

  public var maxX: Double { offset + max(0, width) }
}

public enum WorkspaceTabStickyEdge: Equatable, Sendable {
  case leading
  case trailing
}

public struct WorkspaceTabStickyPresentation: Equatable, Sendable {
  public var workspaceId: String
  public var edge: WorkspaceTabStickyEdge

  public init(workspaceId: String, edge: WorkspaceTabStickyEdge) {
    self.workspaceId = workspaceId
    self.edge = edge
  }
}

public struct WorkspaceTabStripPlan: Equatable, Sendable {
  public var fullyVisibleWorkspaceIds: [String]
  public var hiddenWorkspaceIds: [String]
  public var sticky: WorkspaceTabStickyPresentation?

  public init(
    fullyVisibleWorkspaceIds: [String], hiddenWorkspaceIds: [String],
    sticky: WorkspaceTabStickyPresentation?
  ) {
    self.fullyVisibleWorkspaceIds = fullyVisibleWorkspaceIds
    self.hiddenWorkspaceIds = hiddenWorkspaceIds
    self.sticky = sticky
  }

  public var hiddenCount: Int { hiddenWorkspaceIds.count }
}

public enum WorkspaceTabFocusDirection: Sendable {
  case previous
  case next
}

public enum WorkspaceTabStripPolicy {
  /// Plans visibility from frames in scroll-content coordinates.
  ///
  /// A sticky active tab is visible through its proxy, so it is not counted as hidden even
  /// while its canonical cell is outside the viewport.
  public static func plan(
    items: [WorkspaceTabStripItem], activeWorkspaceId: String?,
    viewport: WorkspaceTabStripViewport
  ) -> WorkspaceTabStripPlan {
    let fullyVisible = items.filter { item in
      item.minX >= viewport.offset && item.maxX <= viewport.maxX
    }
    let fullyVisibleIds = fullyVisible.map(\.workspaceId)
    let visibleSet = Set(fullyVisibleIds)
    let active = items.first { $0.workspaceId == activeWorkspaceId }
    let sticky = active.flatMap { item -> WorkspaceTabStickyPresentation? in
      guard !visibleSet.contains(item.workspaceId) else { return nil }
      let edge: WorkspaceTabStickyEdge = item.minX < viewport.offset ? .leading : .trailing
      return WorkspaceTabStickyPresentation(workspaceId: item.workspaceId, edge: edge)
    }
    let hidden = items.compactMap { item -> String? in
      guard !visibleSet.contains(item.workspaceId), item.workspaceId != sticky?.workspaceId else {
        return nil
      }
      return item.workspaceId
    }
    return WorkspaceTabStripPlan(
      fullyVisibleWorkspaceIds: fullyVisibleIds,
      hiddenWorkspaceIds: hidden,
      sticky: sticky)
  }

  /// Returns the smallest content offset that makes the requested canonical cell whole.
  public static func revealOffset(
    workspaceId: String, items: [WorkspaceTabStripItem],
    viewport: WorkspaceTabStripViewport, contentWidth: Double
  ) -> Double? {
    guard let item = items.first(where: { $0.workspaceId == workspaceId }) else { return nil }
    let target: Double
    if item.width > viewport.width || item.minX < viewport.offset {
      target = item.minX
    } else if item.maxX > viewport.maxX {
      target = item.maxX - viewport.width
    } else {
      return nil
    }
    return min(max(0, target), max(0, contentWidth - max(0, viewport.width)))
  }

  /// Browser convention: closing the active item selects the following one, or the previous
  /// item when the closed item was last.
  public static func selectionAfterClosing(
    workspaceId: String, orderedWorkspaceIds: [String]
  ) -> String? {
    guard let index = orderedWorkspaceIds.firstIndex(of: workspaceId) else { return nil }
    let next = orderedWorkspaceIds.index(after: index)
    if next < orderedWorkspaceIds.endIndex { return orderedWorkspaceIds[next] }
    guard index > orderedWorkspaceIds.startIndex else { return nil }
    return orderedWorkspaceIds[orderedWorkspaceIds.index(before: index)]
  }

  /// Moves keyboard focus in canonical order without changing that order or wrapping at an end.
  public static func focusNeighbor(
    of workspaceId: String?, direction: WorkspaceTabFocusDirection,
    orderedWorkspaceIds: [String]
  ) -> String? {
    guard !orderedWorkspaceIds.isEmpty else { return nil }
    guard let workspaceId,
      let index = orderedWorkspaceIds.firstIndex(of: workspaceId)
    else {
      return direction == .next ? orderedWorkspaceIds.first : orderedWorkspaceIds.last
    }
    switch direction {
    case .previous:
      guard index > orderedWorkspaceIds.startIndex else { return nil }
      return orderedWorkspaceIds[orderedWorkspaceIds.index(before: index)]
    case .next:
      let next = orderedWorkspaceIds.index(after: index)
      return next < orderedWorkspaceIds.endIndex ? orderedWorkspaceIds[next] : nil
    }
  }
}
