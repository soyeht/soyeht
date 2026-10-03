import AppKit
import CoreText
import SoyehtClient
import SoyehtProtocol
import SwiftUI

extension LocalizedStringKey.StringInterpolation {
  mutating func appendInterpolation<Value: BinaryInteger>(identifier value: Value) {
    appendInterpolation(String(value))
  }

  mutating func appendInterpolation(identifier value: String) {
    appendInterpolation(value)
  }
}

@MainActor enum MacTheme {
  static var canvas: Color { SoyehtThemeColors.canvas }
  static var face: Color { SoyehtThemeColors.face }
  static var well: Color { SoyehtThemeColors.well }
  static var hover: Color { SoyehtThemeColors.hover }
  static var border: Color { SoyehtThemeColors.border }
  static var ink: Color { SoyehtThemeColors.ink }
  static var secondary: Color { SoyehtThemeColors.secondary }
  static var muted: Color { SoyehtThemeColors.muted }
  static var accent: Color { SoyehtThemeColors.accent }
  static var onAccent: Color { SoyehtThemeColors.onAccent }
  static var shadowDark: Color { SoyehtThemeColors.shadowDark }
  static var shadowLight: Color { SoyehtThemeColors.shadowLight }
  static var wellShadow: Color { SoyehtThemeColors.wellShadow }
  static var wellRim: Color { SoyehtThemeColors.wellRim }
  static var wellLip: Color? { SoyehtThemeColors.wellLip }
  static var selection: Color { SoyehtThemeColors.selection }
  static var success: Color { SoyehtThemeColors.success }
  static var warning: Color { SoyehtThemeColors.warning }
  static var danger: Color { SoyehtThemeColors.danger }
  static var syntaxType: Color { SoyehtThemeColors.syntaxType }
  static var diffAdded: Color { SoyehtThemeColors.diffAdded }
  static var presentationLetterbox: Color { SoyehtThemeColors.presentationLetterbox }
  static var urgent: Color { danger }
  static var agentRose: Color { SoyehtThemeColors.agentRose }
  static var agentBlue: Color { SoyehtThemeColors.agentBlue }
  static var agentGreen: Color { SoyehtThemeColors.agentGreen }
  static var agentSand: Color { SoyehtThemeColors.agentSand }

  enum Radius {
    static let compact: CGFloat = 10
    static let control: CGFloat = 12
    static let card: CGFloat = 20
    static let pill: CGFloat = 999
  }

  enum Space {
    static let x2: CGFloat = 2
    static let x3: CGFloat = 3
    static let x4: CGFloat = 4
    static let x5: CGFloat = 5
    static let x6: CGFloat = 6
    static let x7: CGFloat = 7
    static let x8: CGFloat = 8
    static let x10: CGFloat = 10
    static let x12: CGFloat = 12
    static let x14: CGFloat = 14
    static let x16: CGFloat = 16
    static let x18: CGFloat = 18
  }

  enum TypeScale {
    static let micro: CGFloat = 10
    static let small: CGFloat = 11
    static let control: CGFloat = 12
    static let caption: CGFloat = 13
    static let body: CGFloat = 15
    static let subheading: CGFloat = 18
    static let heading: CGFloat = 20
    static let title: CGFloat = 28
  }

  struct Elevation: Sendable {
    let lightOffset: CGFloat
    let lightBlur: CGFloat
    let darkOffset: CGFloat
    let darkBlur: CGFloat

    init(offset: CGFloat, blur: CGFloat) {
      lightOffset = offset
      lightBlur = blur
      darkOffset = offset
      darkBlur = blur
    }

    init(lightOffset: CGFloat, lightBlur: CGFloat, darkOffset: CGFloat, darkBlur: CGFloat) {
      self.lightOffset = lightOffset
      self.lightBlur = lightBlur
      self.darkOffset = darkOffset
      self.darkBlur = darkBlur
    }

    static let chip = Elevation(offset: 3, blur: 5)
    static let toggle = Elevation(offset: 2, blur: 4)
    static let taskCard = Elevation(offset: 3, blur: 6)
    static let action = Elevation(offset: 4, blur: 8)
    static let card = Elevation(offset: 4, blur: 9)
    static let pill = Elevation(offset: 6, blur: 10)
    static let panel = Elevation(offset: 9, blur: 18)
    static let balloon = Elevation(lightOffset: 4, lightBlur: 10, darkOffset: 6, darkBlur: 14)
  }

  static func chrome(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom("Nunito", size: size).weight(weight)
  }

  static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom("JetBrains Mono", size: size).weight(weight)
  }

  /// Inter, the interface face of the approved pane interior.
  static func ui(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    PaneInteriorFont.ui(size, weight: weight)
  }

  static func screenplay(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom("Courier Prime", size: size).weight(weight)
  }

  static func prose(
    _ family: WritingFont, _ size: CGFloat, weight: Font.Weight = .regular
  ) -> Font {
    .custom(proseFamily(family), size: size).weight(weight)
  }

  static func proseNSFont(_ family: WritingFont, _ size: CGFloat, bold: Bool = false) -> NSFont {
    let base = NSFont(name: proseFamily(family), size: size) ?? NSFont.systemFont(ofSize: size)
    return bold ? NSFontManager.shared.convert(base, toHaveTrait: .boldFontMask) : base
  }

  static func monoNSFont(_ size: CGFloat, bold: Bool = false) -> NSFont {
    let base =
      NSFont(name: "JetBrains Mono", size: size)
      ?? NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
    return bold ? NSFontManager.shared.convert(base, toHaveTrait: .boldFontMask) : base
  }

  static func screenplayNSFont(_ size: CGFloat, bold: Bool = false) -> NSFont {
    let base = NSFont(name: "Courier Prime", size: size) ?? monoNSFont(size)
    return bold ? NSFontManager.shared.convert(base, toHaveTrait: .boldFontMask) : base
  }

  private static func proseFamily(_ family: WritingFont) -> String {
    switch family {
    case .reading_serif: "Newsreader"
    case .classic_serif: "Times New Roman"
    case .sans: "Nunito"
    }
  }

  static func registerFonts() {
    let fonts = Bundle.main.urls(forResourcesWithExtension: "ttf", subdirectory: "Fonts") ?? []
    for font in fonts {
      _ = CTFontManagerRegisterFontsForURL(font as CFURL, .process, nil)
    }
  }

  static var agentConversation: AgentConversationStyle {
    .pane(SoyehtThemeColorSet(palette: SoyehtThemeColors.palette))
  }
}

private struct NeoRaised: ViewModifier {
  let radius: CGFloat
  let elevation: MacTheme.Elevation

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(MacTheme.face)
        .compositingGroup()
        .shadow(
          color: MacTheme.shadowLight, radius: elevation.lightBlur / 2,
          x: -elevation.lightOffset, y: -elevation.lightOffset
        )
        .shadow(
          color: MacTheme.shadowDark, radius: elevation.darkBlur / 2,
          x: elevation.darkOffset, y: elevation.darkOffset)
    }
  }
}

/// Configures the SwiftUI `ScrollView` that owns the marked content. AppKit otherwise
/// follows the machine-wide scrollbar preference, which can reintroduce the legacy
/// permanent scroller inside writing panes.
private struct EnclosingOverlayScrollerStyle: NSViewRepresentable {
  func makeNSView(context: Context) -> NSView {
    let marker = NSView(frame: .zero)
    configure(marker)
    return marker
  }

  func updateNSView(_ marker: NSView, context: Context) { configure(marker) }

  private func configure(_ marker: NSView) {
    DispatchQueue.main.async { [weak marker] in
      var current: NSView? = marker
      var configured = Set<ObjectIdentifier>()
      while let view = current {
        if let scrollView = view as? NSScrollView ?? view.enclosingScrollView {
          let identity = ObjectIdentifier(scrollView)
          if configured.insert(identity).inserted {
            OverlayScrollers.apply(to: scrollView)
          }
          // Writing panes can place an AppKit text scroll view inside the SwiftUI
          // overflow scroll view. Continue above the nearest owner so both adopt
          // the same non-persistent overlay policy.
          current = scrollView.superview
        } else {
          current = view.superview
        }
      }
    }
  }
}

/// Removes the horizontal indicator from the nearest owner while preserving gesture
/// scrolling. Attach this to content inside a compact horizontal `ScrollView`.
private struct EnclosingHorizontalScrollerHider: NSViewRepresentable {
  func makeNSView(context: Context) -> NSView {
    let marker = NSView(frame: .zero)
    configure(marker)
    return marker
  }

  func updateNSView(_ marker: NSView, context: Context) { configure(marker) }

  private func configure(_ marker: NSView) {
    DispatchQueue.main.async { [weak marker] in
      marker?.enclosingScrollView?.hasHorizontalScroller = false
    }
  }
}

private struct NeoWell: ViewModifier {
  let radius: CGFloat
  let depth: CGFloat

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(
          MacTheme.well
            .shadow(.inner(color: MacTheme.wellShadow, radius: depth, x: depth, y: depth))
            .shadow(.inner(color: MacTheme.wellRim, radius: depth, x: -depth, y: -depth))
        )
        .overlay {
          if let lip = MacTheme.wellLip {
            RoundedRectangle(cornerRadius: radius, style: .continuous)
              .strokeBorder(lip, lineWidth: 1)
          }
        }
    }
  }
}

private struct NeoAccentGlow: ViewModifier {
  let radius: CGFloat

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(MacTheme.accent)
        .shadow(color: MacTheme.accent.opacity(0.35), radius: 5, x: 4.8, y: 4.8)
        .shadow(color: MacTheme.shadowLight, radius: 3, x: -3, y: -3)
    }
  }
}

private struct MacSheetAvailableSizeKey: EnvironmentKey {
  static let defaultValue: PaneLayoutSize? = nil
}

extension EnvironmentValues {
  var macSheetAvailableSize: PaneLayoutSize? {
    get { self[MacSheetAvailableSizeKey.self] }
    set { self[MacSheetAvailableSizeKey.self] = newValue }
  }
}

struct MacSheetSurface<SheetContent: View>: View {
  @Environment(\.macSheetAvailableSize) private var availableSize
  @State private var windowAvailableSize: PaneLayoutSize?
  let kind: MacSheetKind
  @ViewBuilder let content: (MacSheetLayoutPlan) -> SheetContent

  var body: some View {
    let plan = MacSheetLayoutPolicy().plan(
      for: kind, available: windowAvailableSize ?? availableSize)
    content(plan).frame(
      width: CGFloat(plan.width),
      height: plan.height.map { CGFloat($0) }
    )
    .background {
      MacSheetWindowSizeReader { size in
        let measured = PaneLayoutSize(width: size.width, height: size.height)
        if measured != windowAvailableSize { windowAvailableSize = measured }
      }
      .frame(width: 0, height: 0)
    }
  }
}

private struct MacSheetSurfaceModifier: ViewModifier {
  let kind: MacSheetKind

  func body(content: Content) -> some View {
    MacSheetSurface(kind: kind) { _ in content }
  }
}

private struct MacSheetWindowSizeReader: NSViewRepresentable {
  let update: (CGSize) -> Void

  func makeNSView(context: Context) -> NSView {
    let view = NSView(frame: .zero)
    measure(view)
    return view
  }

  func updateNSView(_ view: NSView, context: Context) {
    measure(view)
  }

  private func measure(_ view: NSView) {
    DispatchQueue.main.async { [weak view] in
      guard let window = view?.window else { return }
      update((window.sheetParent ?? window).contentLayoutRect.size)
    }
  }
}

extension View {
  func forceEnclosingOverlayScrollers() -> some View {
    background { EnclosingOverlayScrollerStyle().frame(width: 0, height: 0) }
  }

  func hideEnclosingHorizontalScroller() -> some View {
    background { EnclosingHorizontalScrollerHider().frame(width: 0, height: 0) }
  }

  func neoRaised(
    radius: CGFloat = MacTheme.Radius.card,
    elevation: MacTheme.Elevation = .card
  ) -> some View {
    modifier(NeoRaised(radius: radius, elevation: elevation))
  }

  func neoWell(radius: CGFloat = MacTheme.Radius.control, depth: CGFloat = 3) -> some View {
    modifier(NeoWell(radius: radius, depth: depth))
  }

  func neoAccent(radius: CGFloat = MacTheme.Radius.control) -> some View {
    modifier(NeoAccentGlow(radius: radius))
  }

  func pickerHeading() -> some View {
    font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
      .tracking(1).foregroundStyle(MacTheme.muted)
  }

  func macSheetSurface(_ kind: MacSheetKind) -> some View {
    modifier(MacSheetSurfaceModifier(kind: kind))
  }
}

struct NeoPillButtonStyle: ButtonStyle {
  var accent = false
  var elevation = MacTheme.Elevation.pill

  @ViewBuilder
  func makeBody(configuration: Configuration) -> some View {
    if accent {
      configuration.label
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
        .foregroundStyle(MacTheme.onAccent)
        .padding(.horizontal, MacTheme.Space.x14)
        .padding(.vertical, MacTheme.Space.x8)
        .opacity(configuration.isPressed ? 0.78 : 1)
        .neoAccent(radius: MacTheme.Radius.pill)
    } else {
      configuration.label
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .foregroundStyle(MacTheme.ink)
        .padding(.horizontal, MacTheme.Space.x14)
        .padding(.vertical, MacTheme.Space.x8)
        .opacity(configuration.isPressed ? 0.78 : 1)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: elevation)
    }
  }
}
