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

/// Content rulers accepted in `docs/reports/pane-layout-floor-measurements.md`.
public enum PaneLayoutFloorFormula {
  public static let terminalColumns = 40.0
  public static let terminalRows = 12.0
  public static let editorColumns = 36.0
  public static let agentComposerCharacters = 24.0
  public static let compactTextCharacters = 24.0
  public static let screenplayColumns = 61.0
  public static let minimumScreenplayPointSize = 7.5
  public static let paneCardInset = 10.0

  public static func surfaceFloors(
    fonts: PaneLayoutFontMetrics
  ) -> [PaneLayoutKind: PaneLayoutSize] {
    let terminal = PaneLayoutSize(
      width: ceil(terminalColumns * fonts.terminal.characterWidth + 2 * paneCardInset),
      height: ceil(
        2 * paneCardInset + 26 + 8 + (fonts.chrome.lineHeight + 14)
          + terminalRows * fonts.terminal.lineHeight))
    let editor = PaneLayoutSize(
      width: ceil(editorColumns * fonts.editor.characterWidth + 44 + 20 + 2 * paneCardInset),
      height: 2 * paneCardInset + 27 + 8 + 26 + (12 * 19 + 20) + 42 + 24)
    let agent = PaneLayoutSize(
      width: ceil(agentComposerCharacters * fonts.agent.characterWidth + 124),
      height: 2 * paneCardInset + 26 + 8 + 4 + 34 + 12 + 33 + 1 + 150 + 67)
    let web = PaneLayoutSize(
      width: 320 + 2 * paneCardInset,
      height: 2 * paneCardInset + 26 + 8 + 27 + 8 + 240 + 8 + 24)
    let text = PaneLayoutSize(
      width: ceil(compactTextCharacters * fonts.prose.characterWidth + 32 + 2 * paneCardInset),
      height: 2 * paneCardInset + 28 + 42 + 42 + 3 * 8 + 32 + 6 * 32)
    let screenplayPageWidth = screenplayColumns * fonts.screenplay.characterWidth + 148
    let screenplayZoom = minimumScreenplayPointSize / fonts.screenplay.pointSize
    let screenplayWidth = ceil(screenplayPageWidth * screenplayZoom)
    let screenplay = PaneLayoutSize(
      width: screenplayWidth,
      height: 42 + 60 + 34 + ceil(screenplayWidth * 11 / 8.5))
    let presentation = PaneLayoutSize(
      width: 320 + 2 * paneCardInset,
      height: 42 + 32 + 180 + 28 + 2 * paneCardInset)

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
