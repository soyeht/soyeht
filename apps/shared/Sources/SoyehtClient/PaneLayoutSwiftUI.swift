import SwiftUI

extension Axis.Set {
  /// The axes a pane scrolls along when its content overflows its frame on `overflow`.
  public init(overflow: Set<PaneLayoutAxis>) {
    self = []
    if overflow.contains(.horizontal) { insert(.horizontal) }
    if overflow.contains(.vertical) { insert(.vertical) }
  }
}

extension View {
  /// Places a writing surface in the plan's coordinate space without post-layout clipping.
  public func writingFrame(_ rect: PaneLayoutRect) -> some View {
    frame(width: rect.size.width, height: rect.size.height, alignment: .topLeading)
      .position(
        x: rect.origin.x + rect.size.width / 2,
        y: rect.origin.y + rect.size.height / 2)
  }
}
