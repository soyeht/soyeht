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
  public static let writingHeaderHeight = Double(DesignTokens.PaneLayout.writingHeaderHeight)
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
  /// The shortest stretch of the activity line worth drawing: a header too narrow for it
  /// drops the line (`PaneHeaderLayoutPolicy`), because a ticker rolling through a few
  /// characters reads as garbled text.
  public static let genericPaneHeaderTickerMinimumWidth = Double(
    DesignTokens.PaneLayout.headerTickerMinimumWidth)
  /// "· 59m 59s": the running turn's time, which follows the activity line.
  public static let genericPaneHeaderElapsedCharacters = 10.0
  public static let genericPaneHeaderActionWidth = Double(DesignTokens.PaneLayout.headerActionWidth)
  public static let terminalFontPointSize = Double(DesignTokens.PaneLayout.terminalFontSize)
  public static let terminalStateVerticalInset = Double(
    DesignTokens.PaneLayout.terminalStateVerticalInset)

  public static let editorFontPointSize = Double(DesignTokens.Editor.fontPointSize)
  public static let editorLineHeight = Double(DesignTokens.Editor.lineHeight)
  public static let editorGutterWidth = Double(DesignTokens.Editor.gutterWidth)
  public static let editorTextInset = Double(DesignTokens.Editor.textInset)
  public static let editorHeaderHeight = Double(DesignTokens.Editor.headerHeight)
  public static let editorTabHeight = Double(DesignTokens.Editor.tabHeight)
  public static let editorViewportVerticalInset = Double(DesignTokens.Editor.viewportVerticalInset)
  public static let editorReviewHeight = Double(DesignTokens.PaneLayout.editorReviewHeight)
  public static let editorStatusHeight = Double(DesignTokens.Editor.statusHeight)
  public static let editorFindFieldHeight = Double(DesignTokens.Editor.findFieldHeight)
  /// Lines the replace field up under the find field, past the disclosure.
  public static let editorFindIndent = editorFindFieldHeight + 6.0
  /// The project panel beside the code; part of the editor's full minimum width. A narrow
  /// layout folds it away and lays the editor out at its compact floor.
  public static let editorProjectPanelWidth = Double(DesignTokens.Editor.projectPanelWidth)

  public static let agentTaskHeight = Double(DesignTokens.PaneLayout.agentTaskHeight)
  public static let agentBodyTopInset = Double(DesignTokens.PaneLayout.agentBodyTopInset)
  public static let agentControlsFixtureHeight = Double(
    DesignTokens.PaneLayout.agentControlsFixtureHeight)
  public static let agentDividerHeight = Double(DesignTokens.PaneLayout.agentDividerHeight)
  public static let agentTranscriptFixtureHeight = Double(
    DesignTokens.PaneLayout.agentTranscriptFixtureHeight)
  public static let agentComposerFixtureHeight = Double(
    DesignTokens.PaneLayout.agentComposerFixtureHeight)

  public static let webNavigationHeight = Double(DesignTokens.PaneLayout.webNavigationHeight)
  public static let webViewportFixture = PaneLayoutSize(
    width: Double(DesignTokens.PaneLayout.webViewportWidth),
    height: Double(DesignTokens.PaneLayout.webViewportHeight))
  public static let webStatusHeight = Double(DesignTokens.PaneLayout.webStatusHeight)

  public static let textFontPointSize = 20.0
  public static let textLineHeight = Double(DesignTokens.PaneLayout.textLineHeight)
  public static let textReviewHeight = editorReviewHeight
  public static let textPageInset = Double(DesignTokens.PaneLayout.textPageInset)
  public static let textReadingCharacters = 66.0
  public static let textCompactLines = Double(DesignTokens.PaneLayout.textCompactLines)

  public static let screenplayFontPointSize = 16.0
  public static let screenplayLineHeight = 27.0
  public static let screenplayPageHorizontalInset = Double(
    DesignTokens.PaneLayout.screenplayPageHorizontalInset)
  public static let screenplayPageRatio =
    Double(DesignTokens.PaneLayout.screenplayPageHeightInches)
    / Double(DesignTokens.PaneLayout.screenplayPageWidthInches)
  public static let screenplayMinimumScale =
    Double(DesignTokens.PaneLayout.minimumScreenplayPointSize) / screenplayFontPointSize
  public static let screenplaySceneRailWidth = 330.0
  public static let screenplaySceneDrawerHeight = 420.0
  public static let screenplayInlineElementMenuWidth = 148.0
  public static let screenplayInlineElementMenuHeight = 30.0
  public static let screenplayInlineElementMenuInset = 12.0
  public static let screenplayInlineControlGap = 6.0
  /// Keeps the 30 pt inline controls clear of the preceding 27 pt screenplay line.
  public static let screenplayInlineAnchorClearance = 3.0
  public static let screenplaySmartTypeFontPointSize = 11.0

  public static let presentationNumericRailHeight = Double(
    DesignTokens.PaneLayout.presentationNumericRailHeight)
  public static let presentationStage = PaneLayoutSize(
    width: Double(DesignTokens.PaneLayout.presentationStageWidth),
    height: Double(DesignTokens.PaneLayout.presentationStageHeight))
  public static let presentationMaximumStage = PaneLayoutSize(width: 1024, height: 576)
  public static let presentationThumbnailRailWidth = 236.0
  public static let presentationFooterHeight = Double(
    DesignTokens.PaneLayout.presentationFooterHeight)

  public static let newPaneHeaderHeight = genericPaneHeaderHeight
  public static let newPaneBodyVerticalInset = Double(
    DesignTokens.PaneLayout.newPaneBodyVerticalInset)
  public static let newPaneSectionSpacing = Double(DesignTokens.PaneLayout.newPaneSectionSpacing)
  public static let newPaneHeadingHeight = Double(DesignTokens.PaneLayout.newPaneHeadingHeight)
  public static let newPaneFormHeight = Double(DesignTokens.PaneLayout.newPaneFormHeight)
  public static let newPaneMetadataHeight = Double(DesignTokens.PaneLayout.newPaneMetadataHeight)
  public static let newPaneCompactFooterHeight = Double(
    DesignTokens.PaneLayout.newPaneCompactFooterHeight)
  /// The room the prompt leaves for the shortcut hint at its trailing edge.
  public static let newPanePromptTrailingInset = 54.0
  public static let newPaneAddGlyphSize = 16.0
  public static let newPaneChevronGlyphSize = 10.0
}

/// Content rulers accepted in `docs/reports/pane-layout-floor-measurements.md`.
public enum PaneLayoutFloorFormula {
  public static let terminalColumns = Double(DesignTokens.PaneLayout.terminalColumns)
  public static let terminalRows = Double(DesignTokens.PaneLayout.terminalRows)
  public static let editorColumns = Double(DesignTokens.PaneLayout.editorColumns)
  public static let agentComposerCharacters = Double(
    DesignTokens.PaneLayout.composerCharacters)
  public static let compactTextCharacters = Double(DesignTokens.PaneLayout.compactTextCharacters)
  public static let newPaneComposerCharacters = Double(
    DesignTokens.PaneLayout.composerCharacters)
  public static let newPaneControlChromeWidth = Double(
    DesignTokens.PaneLayout.composerControlChromeWidth)
  public static let screenplayColumns = Double(DesignTokens.PaneLayout.screenplayColumns)
  public static let minimumScreenplayPointSize = Double(
    DesignTokens.PaneLayout.minimumScreenplayPointSize)
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
      width: ceil(
        agentComposerCharacters * fonts.agent.characterWidth
          + Double(DesignTokens.PaneLayout.composerControlChromeWidth)),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.genericPaneHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.agentBodyTopInset
        + PaneLayoutContentMetrics.agentTaskHeight
        + Double(DesignTokens.PaneLayout.agentBodyBottomInset)
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
        + PaneLayoutContentMetrics.textCompactLines * PaneLayoutContentMetrics.textLineHeight
        + paneCardInset)
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

  /// The editor shows the Markdown preview beside its lines only when the text area (the editor
  /// without its project panel) holds the lines at their floor, the divider and the preview at
  /// its narrowest. Narrower, the lines keep the room.
  public static func editorShowsPreview(
    textAreaWidth: Double, fonts: PaneLayoutFontMetrics
  ) -> Bool {
    textAreaWidth
      >= ceil(
        editorCodeFloorWidth(fonts: fonts)
          + Double(DesignTokens.PaneLayout.editorPreviewDividerWidth)
          + Double(DesignTokens.Editor.previewMinWidth))
  }

  /// The width the editor's lines need: their columns, the gutter and the text inset on both sides.
  private static func editorCodeFloorWidth(fonts: PaneLayoutFontMetrics) -> Double {
    editorColumns * fonts.editor.characterWidth + PaneLayoutContentMetrics.editorGutterWidth
      + 2 * PaneLayoutContentMetrics.editorTextInset
  }

  private static func editorSurfaceFloor(
    fonts: PaneLayoutFontMetrics, projectPanel: Bool
  ) -> PaneLayoutSize {
    PaneLayoutSize(
      width: ceil(
        editorCodeFloorWidth(fonts: fonts) + 2 * paneCardInset
          + (projectPanel ? PaneLayoutContentMetrics.editorProjectPanelWidth : 0)),
      height: 2 * paneCardInset + PaneLayoutContentMetrics.editorHeaderHeight
        + PaneLayoutContentMetrics.paneStackSpacing + PaneLayoutContentMetrics.editorTabHeight
        + (Double(DesignTokens.PaneLayout.editorMinimumRows)
          * PaneLayoutContentMetrics.editorLineHeight
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
  /// The widest footprint any pane needs once its optional chrome has folded away.
  public var largestPaneFootprintWidth: Double
  /// The shortest footprint of any pane kind.
  public var smallestPaneFootprintHeight: Double
  public var standardHorizontalInset: Double
  public var topInset: Double
  public var topBarHeight: Double
  public var paneVerticalInset: Double
  public var railWidth: Double

  public init(
    minimumExercisedViewport: PaneLayoutSize, largestPaneFootprintWidth: Double,
    smallestPaneFootprintHeight: Double = 0,
    standardHorizontalInset: Double, topInset: Double, topBarHeight: Double,
    paneVerticalInset: Double, railWidth: Double
  ) {
    self.minimumExercisedViewport = minimumExercisedViewport
    self.largestPaneFootprintWidth = largestPaneFootprintWidth
    self.smallestPaneFootprintHeight = smallestPaneFootprintHeight
    self.standardHorizontalInset = standardHorizontalInset
    self.topInset = topInset
    self.topBarHeight = topBarHeight
    self.paneVerticalInset = paneVerticalInset
    self.railWidth = railWidth
  }

  public var verticalOverhead: Double {
    topInset + topBarHeight + 2 * paneVerticalInset
  }

  /// The narrowest window that holds the open rail beside any pane at its folded floor, with
  /// the standard inset on both sides: below it the rail would be drawn over a pane. The window
  /// is never narrower, so a pane card keeps its whole halo clear of the window's edge too.
  public var minimumWindowWidth: Double {
    largestPaneFootprintWidth + railWidth + 2 * standardHorizontalInset
  }

  /// The shortest window that shows the top bar and the shortest pane whole. A taller pane
  /// scrolls in a window this short.
  public var minimumWindowHeight: Double {
    verticalOverhead + smallestPaneFootprintHeight
  }

  /// The top bar's leading space, measured from the content's edge so the first control starts
  /// the same distance from the window's edge at every width: past the traffic lights, which a
  /// window too narrow for the standard inset would otherwise be drawn over.
  public func topBarLeading(windowWidth: Double, leadingInset: Double) -> Double {
    leadingInset + standardHorizontalInset - horizontalInset(windowWidth: windowWidth)
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
