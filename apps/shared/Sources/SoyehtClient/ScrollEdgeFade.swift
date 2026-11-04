import SwiftUI

extension View {
  /// Fades the edge of a horizontal scroll view that has more behind it, so a chip cut by the
  /// edge reads as "scroll for more" and not as clipped. Where the system cannot say how far the
  /// content scrolled (before iOS 18 and macOS 15) the edges stay as they are.
  @ViewBuilder func fadesCutOffEdges() -> some View {
    if #available(macOS 15, iOS 18, *) {
      modifier(ScrollEdgeFade())
    } else {
      self
    }
  }
}

/// How much of a scroll view's content lies past each of its horizontal edges.
struct ScrollEdgeOverflow: Equatable {
  var leading: Double
  var trailing: Double

  static let none = ScrollEdgeOverflow(leading: 0, trailing: 0)

  /// Less than this is rounding, not a cut-off chip.
  private static let tolerance = 1.0

  init(leading: Double, trailing: Double) {
    self.leading = leading
    self.trailing = trailing
  }

  init(contentWidth: Double, containerWidth: Double, offset: Double) {
    leading = max(0, offset)
    trailing = max(0, contentWidth - containerWidth - offset)
  }

  var hasLeading: Bool { leading > Self.tolerance }
  var hasTrailing: Bool { trailing > Self.tolerance }
}

@available(macOS 15, iOS 18, *)
private struct ScrollEdgeFade: ViewModifier {
  /// The width of the fade.
  private static let fade: CGFloat = 20

  @State private var overflow = ScrollEdgeOverflow.none

  func body(content: Content) -> some View {
    content
      .onScrollGeometryChange(for: ScrollEdgeOverflow.self) { geometry in
        ScrollEdgeOverflow(
          contentWidth: geometry.contentSize.width, containerWidth: geometry.containerSize.width,
          offset: geometry.contentOffset.x)
      } action: { _, new in
        overflow = new
      }
      .mask {
        HStack(spacing: 0) {
          LinearGradient(
            colors: [overflow.hasLeading ? .clear : .black, .black], startPoint: .leading,
            endPoint: .trailing
          )
          .frame(width: Self.fade)
          Rectangle()
          LinearGradient(
            colors: [.black, overflow.hasTrailing ? .clear : .black], startPoint: .leading,
            endPoint: .trailing
          )
          .frame(width: Self.fade)
        }
      }
  }
}
