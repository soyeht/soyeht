import Foundation

public struct PaneLayoutFontMetric: Equatable, Sendable {
  public var characterWidth: Double
  public var lineHeight: Double
  public var pointSize: Double

  public init(characterWidth: Double, lineHeight: Double, pointSize: Double) {
    self.characterWidth = characterWidth
    self.lineHeight = lineHeight
    self.pointSize = pointSize
  }
}

public struct PaneLayoutFontMetrics: Equatable, Sendable {
  public var terminal: PaneLayoutFontMetric
  public var editor: PaneLayoutFontMetric
  public var chrome: PaneLayoutFontMetric
  public var agent: PaneLayoutFontMetric
  public var prose: PaneLayoutFontMetric
  public var screenplay: PaneLayoutFontMetric

  public init(
    terminal: PaneLayoutFontMetric, editor: PaneLayoutFontMetric,
    chrome: PaneLayoutFontMetric, agent: PaneLayoutFontMetric,
    prose: PaneLayoutFontMetric, screenplay: PaneLayoutFontMetric
  ) {
    self.terminal = terminal
    self.editor = editor
    self.chrome = chrome
    self.agent = agent
    self.prose = prose
    self.screenplay = screenplay
  }
}

/// Dimensions shared by the measured floor formulas and the views that own them.
/// Fixture-only rulers remain named here as policy, rather than masquerading as view literals.
public enum PaneLayoutContentMetrics {
  public static let paneCardInset = 10.0
  public static let paneStackSpacing = 8.0

  /// Common writing chrome introduced by the responsive writing policy.
  public static let writingHeaderHeight = 42.0
  public static let writingRailDividerWidth = 1.0
  public static let companionTabStripHeight = 36.0

  public static let genericPaneHeaderHeight = 26.0
  public static let terminalFontPointSize = 13.0
  public static let terminalStateVerticalInset = 7.0

  public static let editorFontPointSize = 11.5
  public static let editorLineHeight = 19.0
  public static let editorGutterWidth = 44.0
  public static let editorTextInset = 10.0
  public static let editorHeaderHeight = 27.0
  public static let editorTabHeight = 26.0
  public static let editorViewportVerticalInset = 20.0
  public static let editorReviewHeight = 42.0
  public static let editorStatusHeight = 24.0

  public static let agentTaskHeight = 34.0
  public static let agentBodyTopInset = 4.0
  public static let agentControlsFixtureHeight = 33.0
  public static let agentDividerHeight = 1.0
  public static let agentTranscriptFixtureHeight = 150.0
  public static let agentComposerFixtureHeight = 67.0

  public static let webNavigationHeight = 27.0
  public static let webViewportFixture = PaneLayoutSize(width: 320, height: 240)
  public static let webStatusHeight = 24.0

  public static let textFontPointSize = 20.0
  public static let textLineHeight = 32.0
  public static let textToolbarHeight = 42.0
  public static let textReviewHeight = editorReviewHeight
  public static let textPageInset = 16.0
  public static let textReadingCharacters = 66.0
  public static let textCompactLines = 6.0

  public static let screenplayFontPointSize = 16.0
  public static let screenplayLineHeight = 27.0
  public static let screenplayPageHorizontalInset = 74.0
  public static let screenplayHeaderHeight = 42.0
  public static let screenplayToolbarHeight = 60.0
  public static let screenplaySmartTypeHeight = 34.0
  public static let screenplayPageRatio = 11.0 / 8.5
  public static let screenplayMinimumScale = 7.5 / screenplayFontPointSize
  public static let screenplaySceneRailWidth = 330.0

  public static let presentationHeaderHeight = 42.0
  public static let presentationNumericRailHeight = 32.0
  public static let presentationStage = PaneLayoutSize(width: 320, height: 180)
  public static let presentationMaximumStage = PaneLayoutSize(width: 1024, height: 576)
  public static let presentationThumbnailRailWidth = 236.0
  public static let presentationFooterHeight = 28.0

  public static let newPaneHeaderHeight = genericPaneHeaderHeight
  public static let newPaneBodyVerticalInset = 12.0
  public static let newPaneSectionSpacing = 18.0
  public static let newPaneHeadingHeight = 24.0
  public static let newPaneFormHeight = 150.0
  public static let newPaneMetadataHeight = 32.0
  public static let newPaneCompactFooterHeight = 64.0
}

/// Content rulers accepted in `docs/reports/pane-layout-floor-measurements.md`.
public enum PaneLayoutFloorFormula {
  public static let terminalColumns = 40.0
  public static let terminalRows = 12.0
  public static let editorColumns = 36.0
  public static let agentComposerCharacters = 24.0
  public static let compactTextCharacters = 24.0
  public static let newPaneComposerCharacters = 24.0
  public static let newPaneControlChromeWidth = 124.0
  public static let screenplayColumns = 61.0
  public static let minimumScreenplayPointSize = 7.5
  public static let paneCardInset = PaneLayoutContentMetrics.paneCardInset

  public static func surfaceFloors(
    fonts: PaneLayoutFontMetrics
  ) -> [PaneLayoutKind: PaneLayoutSize] {
    let terminal = PaneLayoutSize(
      width: ceil(terminalColumns * fonts.terminal.characterWidth + 2 * paneCardInset),
      height: ceil(
        2 * paneCardInset + PaneLayoutContentMetrics.genericPaneHeaderHeight
          + PaneLayoutContentMetrics.paneStackSpacing
          + (fonts.chrome.lineHeight + 2 * PaneLayoutContentMetrics.terminalStateVerticalInset)
          + terminalRows * fonts.terminal.lineHeight))
    let editor = PaneLayoutSize(
      width: ceil(
        editorColumns * fonts.editor.characterWidth + PaneLayoutContentMetrics.editorGutterWidth
          + 2 * PaneLayoutContentMetrics.editorTextInset + 2 * paneCardInset),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.editorHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.editorTabHeight
        + (12 * PaneLayoutContentMetrics.editorLineHeight
          + PaneLayoutContentMetrics.editorViewportVerticalInset)
        + PaneLayoutContentMetrics.editorReviewHeight
        + PaneLayoutContentMetrics.editorStatusHeight)
    let agent = PaneLayoutSize(
      width: ceil(agentComposerCharacters * fonts.agent.characterWidth + 124),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.genericPaneHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.agentBodyTopInset
        + PaneLayoutContentMetrics.agentTaskHeight + 12
        + PaneLayoutContentMetrics.agentControlsFixtureHeight
        + PaneLayoutContentMetrics.agentDividerHeight
        + PaneLayoutContentMetrics.agentTranscriptFixtureHeight
        + PaneLayoutContentMetrics.agentComposerFixtureHeight)
    let web = PaneLayoutSize(
      width: PaneLayoutContentMetrics.webViewportFixture.width + 2 * paneCardInset,
      height: 2 * paneCardInset + PaneLayoutContentMetrics.genericPaneHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing
        + PaneLayoutContentMetrics.webNavigationHeight
        + PaneLayoutContentMetrics.paneStackSpacing
        + PaneLayoutContentMetrics.webViewportFixture.height
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.webStatusHeight)
    let text = PaneLayoutSize(
      width: ceil(
        compactTextCharacters * fonts.prose.characterWidth
          + 2 * PaneLayoutContentMetrics.textPageInset + 2 * paneCardInset),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.writingHeaderHeight
        + PaneLayoutContentMetrics.textToolbarHeight + PaneLayoutContentMetrics.textReviewHeight
        + 3 * PaneLayoutContentMetrics.paneStackSpacing
        + 2 * PaneLayoutContentMetrics.textPageInset
        + 6 * PaneLayoutContentMetrics.textLineHeight)
    let screenplayPageWidth =
      screenplayColumns * fonts.screenplay.characterWidth
      + 2 * PaneLayoutContentMetrics.screenplayPageHorizontalInset
    let screenplayZoom = minimumScreenplayPointSize / fonts.screenplay.pointSize
    let screenplayWidth = ceil(screenplayPageWidth * screenplayZoom)
    let screenplay = PaneLayoutSize(
      width: screenplayWidth,
      height: PaneLayoutContentMetrics.screenplayHeaderHeight
        + PaneLayoutContentMetrics.screenplayToolbarHeight
        + PaneLayoutContentMetrics.screenplaySmartTypeHeight + ceil(screenplayWidth * 11 / 8.5))
    let presentation = PaneLayoutSize(
      width: PaneLayoutContentMetrics.presentationStage.width + 2 * paneCardInset,
      height: PaneLayoutContentMetrics.presentationHeaderHeight
        + PaneLayoutContentMetrics.presentationNumericRailHeight
        + PaneLayoutContentMetrics.presentationStage.height
        + PaneLayoutContentMetrics.presentationFooterHeight + 2 * paneCardInset)

    return [
      .terminal: terminal,
      .editor: editor,
      .agent: agent,
      .web: web,
      .text: text,
      .screenplay: screenplay,
      .presentation: presentation,
      .unknownWriting: screenplay,
    ]
  }

  public static func footprintFloors(
    fonts: PaneLayoutFontMetrics, halo: Double
  ) -> [PaneLayoutKind: PaneLayoutSize] {
    surfaceFloors(fonts: fonts).mapValues {
      PaneLayoutSize(width: ceil($0.width + 2 * halo), height: ceil($0.height + 2 * halo))
    }
  }

  /// The complete compact chooser fixture: header, prompt/mode field, directory/branch metadata,
  /// and the two-line mode switch used at a 375 pt window width.
  public static func newPaneChooserSurfaceFloor(
    fonts: PaneLayoutFontMetrics
  ) -> PaneLayoutSize {
    PaneLayoutSize(
      width: ceil(
        newPaneComposerCharacters * fonts.agent.characterWidth + newPaneControlChromeWidth),
      height: ceil(
        2 * PaneLayoutContentMetrics.paneCardInset
          + PaneLayoutContentMetrics.newPaneHeaderHeight
          + PaneLayoutContentMetrics.paneStackSpacing
          + 2 * PaneLayoutContentMetrics.newPaneBodyVerticalInset
          + PaneLayoutContentMetrics.newPaneHeadingHeight
          + 3 * PaneLayoutContentMetrics.newPaneSectionSpacing
          + PaneLayoutContentMetrics.newPaneFormHeight
          + PaneLayoutContentMetrics.newPaneMetadataHeight
          + PaneLayoutContentMetrics.newPaneCompactFooterHeight))
  }
}

public struct PaneLayoutWindowPolicy: Equatable, Sendable {
  /// The lowest width and height exercised by the accepted floor report.
  public var minimumExercisedViewport: PaneLayoutSize
  public var largestPaneFootprintWidth: Double
  public var standardHorizontalInset: Double
  public var topInset: Double
  public var topBarHeight: Double
  public var paneVerticalInset: Double
  public var railWidth: Double

  public init(
    minimumExercisedViewport: PaneLayoutSize, largestPaneFootprintWidth: Double,
    standardHorizontalInset: Double, topInset: Double, topBarHeight: Double,
    paneVerticalInset: Double, railWidth: Double
  ) {
    self.minimumExercisedViewport = minimumExercisedViewport
    self.largestPaneFootprintWidth = largestPaneFootprintWidth
    self.standardHorizontalInset = standardHorizontalInset
    self.topInset = topInset
    self.topBarHeight = topBarHeight
    self.paneVerticalInset = paneVerticalInset
    self.railWidth = railWidth
  }

  public var verticalOverhead: Double {
    topInset + topBarHeight + 2 * paneVerticalInset
  }

  public func horizontalInset(windowWidth: Double) -> Double {
    min(standardHorizontalInset, max(0, (windowWidth - largestPaneFootprintWidth) / 2))
  }

  public func railOverlays(windowWidth: Double) -> Bool {
    let inset = horizontalInset(windowWidth: windowWidth)
    return windowWidth - 2 * inset - railWidth < largestPaneFootprintWidth
  }

  public func paneViewport(window: PaneLayoutSize, railOpen: Bool) -> PaneLayoutSize {
    let inset = horizontalInset(windowWidth: window.width)
    let rowWidth = max(0, window.width - 2 * inset)
    let consumesWidth = railOpen && !railOverlays(windowWidth: window.width)
    return PaneLayoutSize(
      width: max(0, rowWidth - (consumesWidth ? railWidth : 0)),
      height: max(0, window.height - verticalOverhead))
  }
}
