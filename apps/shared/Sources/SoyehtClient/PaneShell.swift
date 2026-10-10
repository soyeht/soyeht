import SwiftUI

/// The pane shell of the approved frames (design/soyeht-desktop.pen, every pane card): a `face`
/// card with corner radius 20 and two outer shadows, `shadow-dark` at (5, 5) and `shadow-light`
/// at (−5, −5), both with blur 10. It is the only neumorphic surface a pane draws.
public enum PaneShellMetrics {
  public static let cornerRadius: CGFloat = DesignTokens.PaneShell.cornerRadius
  public static let shadowOffset: CGFloat = DesignTokens.PaneShell.shadowOffset
  public static let shadowBlur: CGFloat = DesignTokens.PaneShell.shadowBlur
  /// How far the shell's shadows reach past the card (offset plus blur). The approved frames
  /// draw this whole halo, so nothing around a pane may clip it.
  public static var halo: CGFloat { shadowOffset + shadowBlur }
}

/// Draws the shell behind a pane. SwiftUI's shadow radius is half the design tool's blur: the
/// rendered halo matches the approved frames pixel for pixel at the pane edges.
public struct PaneShell: ViewModifier {
  let face: Color
  let shadowDark: Color
  let shadowLight: Color

  public init(colors: SoyehtThemeColorSet) {
    face = colors.face
    shadowDark = colors.shadowDark
    shadowLight = colors.shadowLight
  }

  public func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: PaneShellMetrics.cornerRadius, style: .continuous)
        .fill(face)
        .compositingGroup()
        .shadow(
          color: shadowLight, radius: PaneShellMetrics.shadowBlur / 2,
          x: -PaneShellMetrics.shadowOffset, y: -PaneShellMetrics.shadowOffset
        )
        .shadow(
          color: shadowDark, radius: PaneShellMetrics.shadowBlur / 2,
          x: PaneShellMetrics.shadowOffset, y: PaneShellMetrics.shadowOffset)
    }
  }
}

extension View {
  public func paneShell(_ colors: SoyehtThemeColorSet) -> some View {
    modifier(PaneShell(colors: colors))
  }
}

/// Hosts laid-out panes. Panes that fit are drawn in place, so every shell's shadow reaches into
/// the gaps and margins around it; only content that overflows sits in a scroll view, which
/// clips by nature. A scroll view must not wrap panes that fit: AppKit clips its document view.
public struct PaneLayoutContainer<Content: View>: View {
  let overflowAxes: Axis.Set
  let size: CGSize
  let contentSize: CGSize
  @ViewBuilder let content: Content

  public init(
    overflowAxes: Axis.Set, size: CGSize, contentSize: CGSize,
    @ViewBuilder content: () -> Content
  ) {
    self.overflowAxes = overflowAxes
    self.size = size
    self.contentSize = contentSize
    self.content = content()
  }

  public var body: some View {
    let placed = content.frame(
      width: contentSize.width, height: contentSize.height, alignment: .topLeading)
    Group {
      if overflowAxes.isEmpty {
        placed
      } else {
        ScrollView(overflowAxes, showsIndicators: true) { placed }
          .scrollBounceBehavior(.basedOnSize)
          .overlayScrollersWithin()
      }
    }
    .frame(width: size.width, height: size.height, alignment: .topLeading)
  }
}
