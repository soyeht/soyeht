public struct PaneLayoutPoint: Equatable, Sendable {
  public var x: Double
  public var y: Double

  public init(x: Double, y: Double) {
    self.x = x
    self.y = y
  }
}

public struct PaneLayoutRect: Equatable, Sendable {
  public var origin: PaneLayoutPoint
  public var size: PaneLayoutSize

  public init(origin: PaneLayoutPoint = PaneLayoutPoint(x: 0, y: 0), size: PaneLayoutSize) {
    self.origin = origin
    self.size = size
  }
}

public struct PaneLayoutNodePath: Hashable, Sendable {
  public var components: [Int]

  public init(components: [Int] = []) {
    self.components = components
  }

  public func appending(_ component: Int) -> PaneLayoutNodePath {
    PaneLayoutNodePath(components: components + [component])
  }
}

public struct PaneLayoutNodeGeometry: Equatable, Sendable {
  /// The node viewport in its parent's scroll-content coordinate space.
  public var frame: PaneLayoutRect
  /// The size retained inside the node's viewport. It never crosses a leaf floor.
  public var contentSize: PaneLayoutSize
  /// Axes whose content is larger than this node's viewport.
  public var overflowAxes: Set<PaneLayoutAxis>

  public init(
    frame: PaneLayoutRect, contentSize: PaneLayoutSize,
    overflowAxes: Set<PaneLayoutAxis> = []
  ) {
    self.frame = frame
    self.contentSize = contentSize
    self.overflowAxes = overflowAxes
  }
}

public struct PaneLayoutGeometryResult: Equatable, Sendable {
  public var nodes: [PaneLayoutNodePath: PaneLayoutNodeGeometry]
  public var panePaths: [String: PaneLayoutNodePath]
  /// Pane footprints in the root content coordinate space at zero scroll offset.
  public var paneRects: [String: PaneLayoutRect]

  public init(
    nodes: [PaneLayoutNodePath: PaneLayoutNodeGeometry],
    panePaths: [String: PaneLayoutNodePath], paneRects: [String: PaneLayoutRect]
  ) {
    self.nodes = nodes
    self.panePaths = panePaths
    self.paneRects = paneRects
  }
}

/// Pure pane geometry. SwiftUI receives these frames and never measures child nodes.
public enum PaneLayoutGeometry {
  public static func layout(
    tree: PaneLayoutTree, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutGeometryResult {
    var builder = Builder(sizing: sizing)
    let rootFrame = PaneLayoutRect(
      size: PaneLayoutSize(width: max(0, viewport.width), height: max(0, viewport.height)))
    try builder.place(
      tree, at: PaneLayoutNodePath(), frame: rootFrame,
      rootOrigin: PaneLayoutPoint(x: 0, y: 0))
    return PaneLayoutGeometryResult(
      nodes: builder.nodes, panePaths: builder.panePaths, paneRects: builder.paneRects)
  }

  public static func layoutRects(
    tree: PaneLayoutTree, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) throws -> [String: PaneLayoutRect] {
    try layout(tree: tree, viewport: viewport, sizing: sizing).paneRects
  }

  private struct Builder {
    let sizing: PaneLayoutSizingPolicy
    var nodes: [PaneLayoutNodePath: PaneLayoutNodeGeometry] = [:]
    var panePaths: [String: PaneLayoutNodePath] = [:]
    var paneRects: [String: PaneLayoutRect] = [:]

    mutating func place(
      _ tree: PaneLayoutTree, at path: PaneLayoutNodePath, frame: PaneLayoutRect,
      rootOrigin: PaneLayoutPoint
    ) throws {
      switch tree {
      case .leaf(let leaf):
        guard let floor = sizing.floors[leaf.kind] else {
          throw PaneLayoutSizingError.missingFloor(leaf.kind)
        }
        let contentSize = PaneLayoutSize(
          width: max(frame.size.width, floor.width),
          height: max(frame.size.height, floor.height))
        var overflow: Set<PaneLayoutAxis> = []
        if contentSize.width > frame.size.width { overflow.insert(.horizontal) }
        if contentSize.height > frame.size.height { overflow.insert(.vertical) }
        nodes[path] = PaneLayoutNodeGeometry(
          frame: frame, contentSize: contentSize, overflowAxes: overflow)
        panePaths[leaf.paneId] = path
        paneRects[leaf.paneId] = PaneLayoutRect(origin: rootOrigin, size: contentSize)

      case .split(let split):
        let minimums = try split.children.map {
          try PaneLayoutMinimumSize.require(
            for: $0.tree, floors: sizing.floors, gaps: sizing.gaps)
        }
        let gap = split.axis == .horizontal ? sizing.gaps.horizontal : sizing.gaps.vertical
        let gapTotal = gap * Double(max(0, split.children.count - 1))
        let viewportExtent = extent(of: frame.size, axis: split.axis)
        let minimumExtent =
          minimums.reduce(0) { $0 + extent(of: $1, axis: split.axis) } + gapTotal
        let contentExtent = max(viewportExtent, minimumExtent)
        let distributable = max(0, contentExtent - minimumExtent)
        let weightTotal = split.children.reduce(0) { $0 + $1.weight }
        let contentSize = replacingExtent(
          in: frame.size, axis: split.axis, with: contentExtent)
        let overflow: Set<PaneLayoutAxis> =
          contentExtent > viewportExtent ? [split.axis] : []
        nodes[path] = PaneLayoutNodeGeometry(
          frame: frame, contentSize: contentSize, overflowAxes: overflow)

        var cursor = 0.0
        for index in split.children.indices {
          let branch = split.children[index]
          let minimum = extent(of: minimums[index], axis: split.axis)
          let share = weightTotal > 0 ? distributable * branch.weight / weightTotal : 0
          let childExtent = minimum + share
          let childSize = replacingExtent(
            in: frame.size, axis: split.axis, with: childExtent)
          let childOrigin = point(axis: split.axis, extent: cursor)
          let childFrame = PaneLayoutRect(origin: childOrigin, size: childSize)
          let childRootOrigin = PaneLayoutPoint(
            x: rootOrigin.x + childOrigin.x, y: rootOrigin.y + childOrigin.y)
          try place(
            branch.tree, at: path.appending(index), frame: childFrame,
            rootOrigin: childRootOrigin)
          cursor += childExtent + gap
        }
      }
    }

    private func extent(of size: PaneLayoutSize, axis: PaneLayoutAxis) -> Double {
      axis == .horizontal ? size.width : size.height
    }

    private func replacingExtent(
      in size: PaneLayoutSize, axis: PaneLayoutAxis, with extent: Double
    ) -> PaneLayoutSize {
      axis == .horizontal
        ? PaneLayoutSize(width: extent, height: size.height)
        : PaneLayoutSize(width: size.width, height: extent)
    }

    private func point(axis: PaneLayoutAxis, extent: Double) -> PaneLayoutPoint {
      axis == .horizontal
        ? PaneLayoutPoint(x: extent, y: 0)
        : PaneLayoutPoint(x: 0, y: extent)
    }
  }
}
