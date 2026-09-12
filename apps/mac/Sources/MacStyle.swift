import SwiftUI

enum Palette {
  static let canvas = Color(red: 224 / 255, green: 229 / 255, blue: 236 / 255)
  static let face = Color(red: 232 / 255, green: 237 / 255, blue: 244 / 255)
  static let ink = Color(red: 62 / 255, green: 74 / 255, blue: 102 / 255)
  static let secondary = Color(red: 110 / 255, green: 122 / 255, blue: 150 / 255)
  static let accent = Color(red: 91 / 255, green: 124 / 255, blue: 250 / 255)
  static let shadow = Color(red: 166 / 255, green: 180 / 255, blue: 200 / 255)
  static let urgent = Color(red: 207 / 255, green: 67 / 255, blue: 92 / 255)
  static let warning = Color(red: 218 / 255, green: 139 / 255, blue: 53 / 255)
  static let success = Color(red: 61 / 255, green: 153 / 255, blue: 111 / 255)
}

struct Raised: ViewModifier {
  var radius: CGFloat = 16

  func body(content: Content) -> some View {
    content.background(Palette.face, in: RoundedRectangle(cornerRadius: radius))
      .shadow(color: .white.opacity(0.85), radius: 8, x: -5, y: -5)
      .shadow(color: Palette.shadow.opacity(0.55), radius: 8, x: 5, y: 5)
  }
}

struct InsetPanel: ViewModifier {
  var radius: CGFloat = 12

  func body(content: Content) -> some View {
    content.background(Palette.canvas, in: RoundedRectangle(cornerRadius: radius))
      .shadow(color: Palette.shadow.opacity(0.42), radius: 4, x: 3, y: 3)
      .shadow(color: .white.opacity(0.72), radius: 4, x: -3, y: -3)
  }
}
