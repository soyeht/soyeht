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
  public static let paneCardInset = Double(DesignTokens.PaneLayout.paneCardInset)
  public static let paneStackSpacing = Double(DesignTokens.PaneLayout.paneStackSpacing)

  /// Common writing chrome measured from the accepted P5/P2 writing-pane design.
  public static let writingHeaderHeight = 40.0
  public static let writingHeaderHorizontalInset = 18.0
  public static let writingHeaderFontPointSize = 14.0
  public static let writingHeaderActionGap = 14.0
  public static let writingHeaderMenuIconSize = 16.0
  public static let writingHeaderMenuTargetSize = 32.0
  public static let writingSelectionToolbarWidth = 232.0
  public static let writingSelectionToolbarHeight = 32.0
  public static let writingSelectionToolbarButtonWidth = 28.0
  public static let writingSelectionToolbarIconSize = 16.0
  public static let writingSelectionToolbarGap = 4.0
  public static let writingSelectionToolbarHorizontalInset = 6.0
  public static let writingSelectionToolbarSelectionGap = 8.0
  public static let writingSelectionToolbarShadowOffset = 4.0
  public static let writingSelectionToolbarShadowBlur = 12.0
  public static let writingSelectionToolbarHalo =
    writingSelectionToolbarShadowOffset + writingSelectionToolbarShadowBlur
  public static let writingChromeDimmedOpacity = 0.35
  public static let writingChromeDimDuration = 0.16
  public static let writingChromeRevealDuration = 0.10
  public static let writingOptionsMenuWidth = 232.0
  public static let writingOptionsRowHeight = 28.0
  public static let writingRailDividerWidth = 1.0
  public static let companionTabStripHeight = 36.0

  public static let genericPaneHeaderHeight = Double(DesignTokens.PaneLayout.headerHeight)
  public static let genericPaneHeaderStateDotWidth = Double(DesignTokens.PaneLayout.headerStateDot)
  public static let genericPaneHeaderReadableTitleCharacters = 8.0
  public static let genericPaneHeaderSummaryCharacters = 12.0
  public static let genericPaneHeaderStatusCharacters = 15.0
  public static let genericPaneHeaderActionWidth = Double(DesignTokens.PaneLayout.headerActionWidth)
  public static let terminalFontPointSize = Double(DesignTokens.PaneLayout.terminalFontSize)
  public static let terminalStateVerticalInset = 7.0

  public static let editorFontPointSize = Double(DesignTokens.Editor.fontPointSize)
  public static let editorLineHeight = Double(DesignTokens.Editor.lineHeight)
  public static let editorGutterWidth = Double(DesignTokens.Editor.gutterWidth)
  public static let editorTextInset = Double(DesignTokens.Editor.textInset)
  public static let editorHeaderHeight = Double(DesignTokens.Editor.headerHeight)
  public static let editorTabHeight = Double(DesignTokens.Editor.tabHeight)
  public static let editorViewportVerticalInset = Double(DesignTokens.Editor.viewportVerticalInset)
  public static let editorReviewHeight = 42.0
  public static let editorStatusHeight = Double(DesignTokens.Editor.statusHeight)
  public static let editorFindFieldHeight = Double(DesignTokens.Editor.findFieldHeight)
  /// Lines the replace field up under the find field, past the disclosure.
  public static let editorFindIndent = editorFindFieldHeight + 6.0
  /// The project panel beside the code; part of the editor's full minimum width. A narrow
  /// layout folds it away and lays the editor out at its compact floor.
  public static let editorProjectPanelWidth = Double(DesignTokens.Editor.projectPanelWidth)

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
  public static let textReviewHeight = editorReviewHeight
  public static let textPageInset = 16.0
  public static let textReadingCharacters = 66.0
  public static let textCompactLines = 6.0

  public static let screenplayFontPointSize = 16.0
  public static let screenplayLineHeight = 27.0
  public static let screenplayPageHorizontalInset = 74.0
  public static let screenplayPageRatio = 11.0 / 8.5
  public static let screenplayMinimumScale = 7.5 / screenplayFontPointSize
  public static let screenplaySceneRailWidth = 330.0
  public static let screenplaySceneDrawerHeight = 420.0
  public static let screenplayInlineElementMenuWidth = 148.0
  public static let screenplayInlineElementMenuHeight = 30.0
  public static let screenplayInlineElementMenuInset = 12.0
  public static let screenplayInlineControlGap = 6.0
  /// Keeps the 30 pt inline controls clear of the preceding 27 pt screenplay line.
  public static let screenplayInlineAnchorClearance = 3.0
  public static let screenplaySmartTypeFontPointSize = 11.0

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
    let editor = editorSurfaceFloor(fonts: fonts, projectPanel: true)
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
      height: PaneLayoutContentMetrics.writingHeaderHeight
        + PaneLayoutContentMetrics.textReviewHeight
        + 2 * PaneLayoutContentMetrics.paneStackSpacing
        + 2 * PaneLayoutContentMetrics.textPageInset
        + 6 * PaneLayoutContentMetrics.textLineHeight + paneCardInset)
    let screenplayPageWidth =
      screenplayColumns * fonts.screenplay.characterWidth
      + 2 * PaneLayoutContentMetrics.screenplayPageHorizontalInset
    let screenplayZoom = minimumScreenplayPointSize / fonts.screenplay.pointSize
    let screenplayWidth = ceil(screenplayPageWidth * screenplayZoom)
    let screenplay = PaneLayoutSize(
      width: screenplayWidth,
      height: PaneLayoutContentMetrics.writingHeaderHeight
        + ceil(screenplayWidth * PaneLayoutContentMetrics.screenplayPageRatio))
    let presentation = PaneLayoutSize(
      width: PaneLayoutContentMetrics.presentationStage.width + 2 * paneCardInset,
      height: PaneLayoutContentMetrics.writingHeaderHeight
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
    surfaceFloors(fonts: fonts).mapValues { footprint($0, halo: halo) }
  }

  /// Floors of panes that can fold optional chrome away when the viewport is narrow: the editor
  /// without its project panel. `PaneLayoutFit` uses them only when the full floors do not fit.
  public static func compactFootprintFloors(
    fonts: PaneLayoutFontMetrics, halo: Double
  ) -> [PaneLayoutKind: PaneLayoutSize] {
    [.editor: footprint(editorSurfaceFloor(fonts: fonts, projectPanel: false), halo: halo)]
  }

  /// The editor shows its project panel only when its surface is as wide as its full floor.
  public static func editorShowsProjectPanel(
    surfaceWidth: Double, fonts: PaneLayoutFontMetrics
  ) -> Bool {
    surfaceWidth >= editorSurfaceFloor(fonts: fonts, projectPanel: true).width
  }

  private static func editorSurfaceFloor(
    fonts: PaneLayoutFontMetrics, projectPanel: Bool
  ) -> PaneLayoutSize {
    PaneLayoutSize(
      width: ceil(
        editorColumns * fonts.editor.characterWidth + PaneLayoutContentMetrics.editorGutterWidth
          + 2 * PaneLayoutContentMetrics.editorTextInset + 2 * paneCardInset
          + (projectPanel ? PaneLayoutContentMetrics.editorProjectPanelWidth : 0)),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.editorHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.editorTabHeight
        + (12 * PaneLayoutContentMetrics.editorLineHeight
          + PaneLayoutContentMetrics.editorViewportVerticalInset)
        + PaneLayoutContentMetrics.editorReviewHeight
        + PaneLayoutContentMetrics.editorStatusHeight)
  }

  public static func footprint(_ surface: PaneLayoutSize, halo: Double) -> PaneLayoutSize {
    PaneLayoutSize(width: ceil(surface.width + 2 * halo), height: ceil(surface.height + 2 * halo))
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
