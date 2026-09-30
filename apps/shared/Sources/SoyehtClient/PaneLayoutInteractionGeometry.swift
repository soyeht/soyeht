public struct PaneLayoutDropTarget: Equatable, Sendable {
  public var paneId: String
  public var workspaceId: String
  public var zone: PaneLayoutDropZone

  public init(paneId: String, workspaceId: String, zone: PaneLayoutDropZone) {
    self.paneId = paneId
    self.workspaceId = workspaceId
    self.zone = zone
  }
}

public enum PaneLayoutDropSurface: Equatable, Sendable {
  case pane
  case chooser
  case workspace
}

public struct PaneLayoutDropPresentation: Equatable, Sendable {
  public var acceptsDrop: Bool
  public var showsTarget: Bool
  public var completesAsNoOp: Bool

  public init(acceptsDrop: Bool, showsTarget: Bool, completesAsNoOp: Bool) {
    self.acceptsDrop = acceptsDrop
    self.showsTarget = showsTarget
    self.completesAsNoOp = completesAsNoOp
  }
}

public enum PaneLayoutDropPresentationPolicy {
  public static func plan(for surface: PaneLayoutDropSurface) -> PaneLayoutDropPresentation {
    switch surface {
    case .pane:
      PaneLayoutDropPresentation(acceptsDrop: true, showsTarget: true, completesAsNoOp: false)
    case .chooser:
      PaneLayoutDropPresentation(acceptsDrop: true, showsTarget: false, completesAsNoOp: true)
    case .workspace:
      PaneLayoutDropPresentation(acceptsDrop: false, showsTarget: false, completesAsNoOp: true)
    }
  }
}

/// Pane moves are local to the Mac app; their pasteboard representation must not be text.
public enum PaneDragTransferPolicy {
  public static let typeIdentifier = "com.soyeht.pane-drag"

  public static func accepts(_ typeIdentifiers: some Sequence<String>) -> Bool {
    typeIdentifiers.contains(typeIdentifier)
  }
}

/// Pure hit testing shared by drag previews and committed drops.
public enum PaneLayoutDropGeometry {
  public static let edgeFraction = 0.25

  public static func target(
    at point: PaneLayoutPoint, sourceWorkspaceId: String, targetWorkspaceId: String,
    targetPaneId: String, paneArea: PaneLayoutRect, targetPaneRect: PaneLayoutRect
  ) -> PaneLayoutDropTarget? {
    guard sourceWorkspaceId == targetWorkspaceId,
      paneArea.contains(point), targetPaneRect.contains(point),
      targetPaneRect.size.width > 0, targetPaneRect.size.height > 0
    else { return nil }

    let horizontalStart = (point.x - targetPaneRect.minX) / targetPaneRect.size.width
    let horizontalEnd = (targetPaneRect.maxX - point.x) / targetPaneRect.size.width
    let verticalStart = (point.y - targetPaneRect.minY) / targetPaneRect.size.height
    let verticalEnd = (targetPaneRect.maxY - point.y) / targetPaneRect.size.height
    let candidates: [(zone: PaneLayoutDropZone, distance: Double, rank: Int)] = [
      (.left, horizontalStart, 0), (.right, horizontalEnd, 1),
      (.top, verticalStart, 2), (.bottom, verticalEnd, 3),
    ]
    let nearest =
      candidates
      .filter { $0.distance <= edgeFraction }
      .min {
        $0.distance == $1.distance ? $0.rank < $1.rank : $0.distance < $1.distance
      }
    return PaneLayoutDropTarget(
      paneId: targetPaneId, workspaceId: targetWorkspaceId,
      zone: nearest?.zone ?? .center)
  }

  public static func rect(for zone: PaneLayoutDropZone, in paneRect: PaneLayoutRect)
    -> PaneLayoutRect
  {
    let edgeWidth = paneRect.size.width * edgeFraction
    let edgeHeight = paneRect.size.height * edgeFraction
    switch zone {
    case .left:
      return PaneLayoutRect(
        origin: paneRect.origin,
        size: PaneLayoutSize(width: edgeWidth, height: paneRect.size.height))
    case .right:
      return PaneLayoutRect(
        origin: PaneLayoutPoint(x: paneRect.maxX - edgeWidth, y: paneRect.minY),
        size: PaneLayoutSize(width: edgeWidth, height: paneRect.size.height))
    case .top:
      return PaneLayoutRect(
        origin: paneRect.origin,
        size: PaneLayoutSize(width: paneRect.size.width, height: edgeHeight))
    case .bottom:
      return PaneLayoutRect(
        origin: PaneLayoutPoint(x: paneRect.minX, y: paneRect.maxY - edgeHeight),
        size: PaneLayoutSize(width: paneRect.size.width, height: edgeHeight))
    case .center:
      return PaneLayoutRect(
        origin: PaneLayoutPoint(x: paneRect.minX + edgeWidth, y: paneRect.minY + edgeHeight),
        size: PaneLayoutSize(
          width: max(0, paneRect.size.width - 2 * edgeWidth),
          height: max(0, paneRect.size.height - 2 * edgeHeight)))
    }
  }
}

extension PaneLayoutRect {
  public var minX: Double { origin.x }
  public var maxX: Double { origin.x + max(0, size.width) }
  public var minY: Double { origin.y }
  public var maxY: Double { origin.y + max(0, size.height) }

  public func contains(_ point: PaneLayoutPoint) -> Bool {
    point.x >= minX && point.x <= maxX && point.y >= minY && point.y <= maxY
  }
}
