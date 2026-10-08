import SwiftUI

/// The Soyeht mark: bold chevrons on a raised rounded square. It is the app icon and the logo
/// shown in the interface, drawn from one description so the two cannot drift. The proportions
/// are those of the welcome screen of the first iOS app: a 96-point face with corner radius 20,
/// flat 34-point bold `chevron.right` glyphs in the accent, and the hero elevation (offset 12,
/// blur 24), shown on a 220-point canvas.
public enum SoyehtMarkMetrics {
  public static let faceSide: CGFloat = 96
  public static let cornerRadius: CGFloat = 20
  public static let glyphSize: CGFloat = 34
  public static let elevationOffset: CGFloat = 12
  public static let elevationBlur: CGFloat = 24
  public static let glyphSymbol = "chevron.right"
  /// The side of the canvas the approved image shows the face on.
  public static let canvasSide: CGFloat = 220

  /// The chevron's size as a fraction of the face side.
  public static var glyphProportion: CGFloat { glyphSize / faceSide }
  /// The corner radius as a fraction of the face side.
  public static var cornerProportion: CGFloat { cornerRadius / faceSide }
  /// The face's side as a fraction of the approved canvas.
  public static var faceOfCanvas: CGFloat { faceSide / canvasSide }
}

/// What varies between the logo shown in the interface and the app icon. The spacing is in
/// points of the 96-point face and scales with it.
public struct SoyehtMarkStyle: Sendable {
  public let chevronCount: Int
  public let chevronSpacing: CGFloat
  public let cornerStyle: RoundedCornerStyle

  /// The logo in the interface: one chevron.
  public static let logo = SoyehtMarkStyle(
    chevronCount: 1, chevronSpacing: 0, cornerStyle: .continuous)
  /// The app icon: two chevrons side by side, drawn exactly as the approved image.
  public static let icon = SoyehtMarkStyle(
    chevronCount: 2, chevronSpacing: -6, cornerStyle: .circular)
}

/// The mark at any size. `side` scales the face, the corner radius, the chevrons and the shadows
/// together. The chevrons are flat: only the face is raised. SwiftUI's shadow radius is half the
/// design tool's blur, as in `PaneShell`.
public struct SoyehtMark: View {
  let colors: SoyehtThemeColorSet
  let side: CGFloat
  let style: SoyehtMarkStyle

  public init(
    colors: SoyehtThemeColorSet, side: CGFloat = SoyehtMarkMetrics.faceSide,
    style: SoyehtMarkStyle = .logo
  ) {
    self.colors = colors
    self.side = side
    self.style = style
  }

  /// The mark in the active theme, for the interface.
  @MainActor public init(side: CGFloat = SoyehtMarkMetrics.faceSide) {
    self.init(colors: SoyehtThemeColorSet(palette: SoyehtThemeColors.palette), side: side)
  }

  public var body: some View {
    let scale = side / SoyehtMarkMetrics.faceSide
    let offset = SoyehtMarkMetrics.elevationOffset * scale
    let radius = SoyehtMarkMetrics.elevationBlur * scale / 2
    HStack(spacing: style.chevronSpacing * scale) {
      ForEach(0..<style.chevronCount, id: \.self) { _ in
        Image(systemName: SoyehtMarkMetrics.glyphSymbol)
      }
    }
    .font(.system(size: side * SoyehtMarkMetrics.glyphProportion, weight: .bold))
    .foregroundStyle(colors.accent)
    .frame(width: side, height: side)
    .background {
      RoundedRectangle(
        cornerRadius: side * SoyehtMarkMetrics.cornerProportion, style: style.cornerStyle
      )
      .fill(colors.face)
      .shadow(color: colors.shadowDark, radius: radius, x: offset, y: offset)
      .shadow(color: colors.shadowLight, radius: radius, x: -offset, y: -offset)
    }
    .accessibilityHidden(true)
  }
}
