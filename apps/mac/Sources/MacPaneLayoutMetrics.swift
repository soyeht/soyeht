import AppKit
import CoreText
import SoyehtClient
import SoyehtProtocol

@MainActor enum MacPaneLayoutMetrics {
  /// Named from the smallest width and height exercised by the accepted floor report.
  static let minimumExercisedViewport = PaneLayoutSize(width: 375, height: 375)
  static let topBarHeight = 42.0
  static let workspaceTabHeight = 36.0
  static let topBarControlHeight = 34.0
  static let workspaceTabMaximumWidth = 216.0
  static let workspaceOverflowButtonWidth = 44.0
  static let workspaceOverflowPopoverWidth = 340.0
  static let workspaceOverflowPopoverMaximumHeight = 420.0
  static let workspaceTabFadeWidth = Double(MacTheme.Space.x18)
  static let railWidth = 56.0
  static let railButtonSize = 36.0

  /// Pane cards fill their footprints; their shadows share the gap (`PaneLayoutSpacing`).
  static let paneHalo = PaneLayoutSpacing.paneHalo
  static let workspaceTabShadowClearance: Double = {
    let elevation = MacTheme.Elevation.pill
    return Double(
      max(
        elevation.lightOffset + elevation.lightBlur,
        elevation.darkOffset + elevation.darkBlur))
  }()
  static let workspaceTabViewportHeight =
    workspaceTabHeight + 2 * workspaceTabShadowClearance

  static let surfaceFloors = PaneLayoutFloorFormula.surfaceFloors(fonts: fontMetrics)
  static let footprintFloors = PaneLayoutFloorFormula.footprintFloors(
    fonts: fontMetrics, halo: paneHalo)
  static let newPaneSurfaceFloor = PaneLayoutFloorFormula.newPaneChooserSurfaceFloor(
    fonts: fontMetrics)
  static let newPaneFootprintFloor = PaneLayoutFloorFormula.footprint(
    newPaneSurfaceFloor, halo: paneHalo)
  static let sizing = PaneLayoutSizingPolicy(
    floors: footprintFloors,
    gaps: PaneLayoutSpacing.gaps,
    compactFloors: PaneLayoutFloorFormula.compactFootprintFloors(fonts: fontMetrics, halo: paneHalo)
  )
  /// The floors `PaneLayoutFit` lays the stored tree out with in this viewport, so divider
  /// gestures clamp against the floors on screen.
  static func fittedSizing(
    _ tree: PaneLayoutTree, viewport: CGSize, projectPanel: EditorProjectPanelPreference
  ) -> PaneLayoutSizingPolicy {
    let size = PaneLayoutSize(width: viewport.width, height: viewport.height)
    let fit = try? PaneLayoutFit.layout(
      tree: tree, viewport: size, sizing: sizing, projectPanel: projectPanel)
    return fit?.sizing ?? sizing
  }
  static func editorShowsProjectPanel(cardWidth: CGFloat) -> Bool {
    PaneLayoutFloorFormula.editorShowsProjectPanel(
      surfaceWidth: Double(cardWidth), fonts: fontMetrics)
  }
  static let chromePolicy = WritingChromePolicy()
  static func sizing(newPanePlaceholderId: String) -> PaneLayoutSizingPolicy {
    sizing.overridingFloor(forPaneId: newPanePlaceholderId, with: newPaneFootprintFloor)
  }
  static let window = PaneLayoutWindowPolicy(
    minimumExercisedViewport: minimumExercisedViewport,
    largestPaneFootprintWidth: footprintFloors.values.map(\.width).max() ?? 0,
    standardHorizontalInset: PaneLayoutSpacing.outerMargin,
    topInset: Double(MacTheme.Space.x8), topBarHeight: topBarHeight,
    paneVerticalInset: PaneLayoutSpacing.outerMargin, railWidth: railWidth)

  static let paneHeader = PaneHeaderLayoutPolicy(
    metrics: PaneHeaderLayoutMetrics(
      horizontalPadding: 2 * Double(MacTheme.Space.x12),
      spacing: Double(MacTheme.Space.x7),
      stateDotWidth: PaneLayoutContentMetrics.genericPaneHeaderStateDotWidth,
      readableTitleWidth: ceil(
        PaneLayoutContentMetrics.genericPaneHeaderReadableTitleCharacters
          * metric(
            font(named: PaneInteriorFont.uiFamily, size: MacTheme.TypeScale.body), character: "0"
          ).characterWidth),
      summaryWidth: ceil(
        PaneLayoutContentMetrics.genericPaneHeaderSummaryCharacters
          * metric(
            font(named: MacTheme.monoFamily, size: MacTheme.TypeScale.micro), character: "0"
          ).characterWidth),
      statusWidth: ceil(
        PaneLayoutContentMetrics.genericPaneHeaderStatusCharacters
          * metric(
            font(named: MacTheme.monoFamily, size: MacTheme.TypeScale.micro), character: "0"
          ).characterWidth),
      actionWidth: PaneLayoutContentMetrics.genericPaneHeaderActionWidth))

  static func paneHeaderAvailableWidth(cardWidth: CGFloat) -> Double {
    max(
      0,
      Double(cardWidth) - 2 * PaneLayoutContentMetrics.paneCardInset
        - paneHeader.metrics.horizontalPadding)
  }

  private static let fontMetrics = PaneLayoutFontMetrics(
    terminal: metric(
      MacTerminalFont.active(),
      character: "W",
      snapToDisplayPixels: true),
    editor: metric(
      font(named: MacTheme.monoFamily, size: PaneLayoutContentMetrics.editorFontPointSize),
      character: "0"),
    chrome: metric(
      font(named: MacTheme.chromeFamily, size: MacTheme.TypeScale.control), character: "0"),
    agent: metric(
      font(named: PaneInteriorFont.uiFamily, size: PaneInteriorMetrics.bodyPointSize),
      character: "0"),
    prose: proseMetric(for: .reading_serif),
    screenplay: screenplayMetric)

  static let screenplayMetric = metric(
    font(named: MacTheme.screenplayFamily, size: PaneLayoutContentMetrics.screenplayFontPointSize),
    character: "0")

  static func proseMetric(for font: WritingFont) -> PaneLayoutFontMetric {
    let measured = metric(
      MacTheme.proseNSFont(font, PaneLayoutContentMetrics.textFontPointSize),
      character: "0")
    return PaneLayoutFontMetric(
      characterWidth: measured.characterWidth,
      lineHeight: PaneLayoutContentMetrics.textLineHeight,
      pointSize: PaneLayoutContentMetrics.textFontPointSize)
  }

  private static func font(named name: String, size: CGFloat) -> NSFont {
    guard let font = NSFont(name: name, size: size) else {
      assertionFailure("Missing bundled pane-layout font: \(name)")
      return NSFont.systemFont(ofSize: size)
    }
    return font
  }

  private static func metric(
    _ font: NSFont, character: String, snapToDisplayPixels: Bool = false
  ) -> PaneLayoutFontMetric {
    let coreTextFont = font as CTFont
    var characterWidth = (character as NSString).size(withAttributes: [.font: font]).width
    var lineHeight = ceil(
      CTFontGetAscent(coreTextFont) + CTFontGetDescent(coreTextFont)
        + CTFontGetLeading(coreTextFont))
    if snapToDisplayPixels {
      let scale = NSScreen.main?.backingScaleFactor ?? 2
      characterWidth = (characterWidth * scale).rounded() / scale
      lineHeight = ceil(lineHeight * scale) / scale
    }
    return PaneLayoutFontMetric(
      characterWidth: Double(characterWidth), lineHeight: Double(lineHeight),
      pointSize: Double(font.pointSize))
  }
}
