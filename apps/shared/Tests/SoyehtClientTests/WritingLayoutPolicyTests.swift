import Foundation
import Testing

@testable import SoyehtClient

private let writingFontMetrics = PaneLayoutFontMetrics(
  terminal: PaneLayoutFontMetric(characterWidth: 8, lineHeight: 18, pointSize: 13),
  editor: PaneLayoutFontMetric(characterWidth: 6.9, lineHeight: 19, pointSize: 11.5),
  chrome: PaneLayoutFontMetric(characterWidth: 7.2, lineHeight: 17, pointSize: 12),
  agent: PaneLayoutFontMetric(characterWidth: 9, lineHeight: 21, pointSize: 15),
  prose: PaneLayoutFontMetric(characterWidth: 11.037, lineHeight: 32, pointSize: 20),
  screenplay: PaneLayoutFontMetric(characterWidth: 9.59375, lineHeight: 27, pointSize: 16))

private let writingFloors = PaneLayoutFloorFormula.surfaceFloors(fonts: writingFontMetrics)

private let writingPolicy = WritingLayoutPolicy(
  floors: writingFloors,
  gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))

private let proseMetric = PaneLayoutFontMetric(
  characterWidth: 11.037, lineHeight: 32, pointSize: 20)
private let screenplayMetric = PaneLayoutFontMetric(
  characterWidth: 9.59375, lineHeight: 27, pointSize: 16)

@Test func textColumnUsesSixtySixCharactersOfTheActiveFontAndFullWidthRemovesOnlyTheCap() throws {
  let viewport = PaneLayoutSize(width: 1_000, height: 700)
  for characterWidth in [11.037_048_339_843_75, 10.0, 12.0] {
    let font = PaneLayoutFontMetric(
      characterWidth: characterWidth, lineHeight: 32, pointSize: 20)
    let reading = try writingPolicy.layout(
      WritingLayoutRequest(kind: .text, viewport: viewport, font: font))
    let full = try writingPolicy.layout(
      WritingLayoutRequest(
        kind: .text, viewport: viewport, font: font, fullWidth: true))
    guard case .text(let readingText) = reading.surface,
      case .text(let fullText) = full.surface
    else {
      Issue.record("Expected text plans")
      return
    }
    #expect(abs(readingText.visibleCharacters - 66) < 0.000_001)
    #expect(abs(readingText.columnRect.size.width - 66 * characterWidth) < 0.000_001)
    #expect(readingText.columnRect.origin.x > 0)
    #expect(fullText.mode == .fullWidth)
    #expect(fullText.columnRect.size.width > readingText.columnRect.size.width)
    #expect(fullText.columnRect.size.width == full.contentRect.size.width - 32)
  }
}

@Test func textFloorIsDerivedFromEachActiveFontMetric() throws {
  let fonts = [
    PaneLayoutFontMetric(characterWidth: 11.037_048_339_843_75, lineHeight: 32, pointSize: 20),
    PaneLayoutFontMetric(characterWidth: 10, lineHeight: 32, pointSize: 20),
    PaneLayoutFontMetric(characterWidth: 12, lineHeight: 32, pointSize: 20),
  ]
  let expectedWidths = [317.0, 317.0, 340.0]

  for (font, expectedWidth) in zip(fonts, expectedWidths) {
    let measured = try #require(WritingLayoutMetrics.standard.surfaceFloor(kind: .text, font: font))
    #expect(measured.width == ceil(24 * font.characterWidth + 52))
    #expect(measured.height == 394)
    let plan = try writingPolicy.layout(
      WritingLayoutRequest(
        kind: .text,
        viewport: PaneLayoutSize(width: expectedWidth - 1, height: measured.height),
        font: font, reviewVisible: true))
    #expect(plan.contentSize.width == expectedWidth)
    #expect(plan.overflowAxes.contains(.horizontal))
  }
}

@Test func compactTextReflowsToItsMeasuredCharacterFloorAndThenScrolls() throws {
  let atFloor = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .text, viewport: writingFloors[.text]!, font: proseMetric,
      reviewVisible: true))
  let belowFloor = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .text,
      viewport: PaneLayoutSize(width: 316, height: 393),
      font: proseMetric, reviewVisible: true))
  guard case .text(let text) = atFloor.surface else {
    Issue.record("Expected text plan")
    return
  }
  #expect(text.visibleCharacters >= 24)
  #expect(text.visibleCharacters < 24.1)
  #expect(atFloor.overflowAxes.isEmpty)
  #expect(belowFloor.overflowAxes == [.horizontal, .vertical])
  #expect(belowFloor.contentSize == writingFloors[.text])
}

@Test func screenplayKeepsOneFixedPageAndMovesItsSceneRailToADrawer() throws {
  let compact = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .screenplay, viewport: writingFloors[.screenplay]!,
      font: screenplayMetric))
  guard case .screenplay(let compactScreenplay) = compact.surface else {
    Issue.record("Expected screenplay plan")
    return
  }
  #expect(compactScreenplay.navigation == .drawer)
  #expect(compactScreenplay.navigationRect == nil)
  #expect(compactScreenplay.pageSize.width == 61 * screenplayMetric.characterWidth + 2 * 74)
  #expect(
    compactScreenplay.pageSize.height
      == compactScreenplay.pageSize.width * WritingLayoutMetrics.standard.screenplayPageRatio)
  #expect(
    compact.toolbarRect?.size.height
      == WritingLayoutMetrics.standard.screenplayToolbarHeight
      + WritingLayoutMetrics.standard.screenplaySmartTypeHeight)
  #expect(
    abs(compactScreenplay.pageRect.size.width / compactScreenplay.pageRect.size.height - 8.5 / 11)
      < 0.000_001)
  #expect(compactScreenplay.scale >= WritingLayoutMetrics.standard.screenplayMinimumScale)
  #expect(compactScreenplay.scale < 0.472)

  let basePageWidth = 61 * screenplayMetric.characterWidth + 2 * 74
  let railThreshold = 330 + 1 + basePageWidth
  let rail = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .screenplay,
      viewport: PaneLayoutSize(width: railThreshold, height: 1_200),
      font: screenplayMetric))
  let onePointShort = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .screenplay,
      viewport: PaneLayoutSize(width: railThreshold - 1, height: 1_200),
      font: screenplayMetric))
  guard case .screenplay(let railScreenplay) = rail.surface,
    case .screenplay(let shortScreenplay) = onePointShort.surface
  else {
    Issue.record("Expected screenplay plans")
    return
  }
  #expect(railScreenplay.navigation == .rail)
  #expect(shortScreenplay.navigation == .drawer)
  #expect(railScreenplay.scale == 1)
}

@Test func screenplayNeverShrinksBelowTheMeasuredRasterScale() throws {
  let plan = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .screenplay,
      viewport: PaneLayoutSize(width: 343, height: 581),
      font: screenplayMetric))
  guard case .screenplay(let screenplay) = plan.surface else {
    Issue.record("Expected screenplay plan")
    return
  }
  #expect(screenplay.scale >= 0.46875)
  #expect(plan.overflowAxes == [.horizontal, .vertical])
}

@Test func presentationStageIsAlwaysSixteenByNineAndNavigationHasAOnePointBoundary() throws {
  let compact = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .presentation, viewport: writingFloors[.presentation]!,
      font: proseMetric))
  guard case .presentation(let compactPresentation) = compact.surface else {
    Issue.record("Expected presentation plan")
    return
  }
  #expect(compactPresentation.navigation == .numbers)
  #expect(compactPresentation.stageRect.size == PaneLayoutSize(width: 320, height: 180))
  #expect(compactPresentation.footerRect.size.height == 28)
  #expect(
    compactPresentation.footerRect.origin.y
      + compactPresentation.footerRect.size.height == compact.contentSize.height)
  let compactRatio =
    compactPresentation.stageRect.size.width / compactPresentation.stageRect.size.height
  #expect(abs(compactRatio - 16.0 / 9.0) < 0.000_001)

  let threshold = 236.0 + 1.0 + 2.0 * 10.0 + 320.0
  let thumbnails = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .presentation,
      viewport: PaneLayoutSize(width: threshold, height: 812), font: proseMetric))
  let onePointShort = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .presentation,
      viewport: PaneLayoutSize(width: threshold - 1, height: 812), font: proseMetric))
  guard case .presentation(let wide) = thumbnails.surface,
    case .presentation(let short) = onePointShort.surface
  else {
    Issue.record("Expected presentation plans")
    return
  }
  #expect(wide.navigation == .thumbnails)
  #expect(short.navigation == .numbers)
  let wideRatio = wide.stageRect.size.width / wide.stageRect.size.height
  #expect(abs(wideRatio - 16.0 / 9.0) < 0.000_001)
}

@Test func presentationStageGrowsUniformlyAndStopsAtItsNamedMaximum() throws {
  let plan = try writingPolicy.layout(
    WritingLayoutRequest(
      kind: .presentation,
      viewport: PaneLayoutSize(width: 1_440, height: 900), font: proseMetric))
  guard case .presentation(let presentation) = plan.surface else {
    Issue.record("Expected presentation plan")
    return
  }
  #expect(presentation.stageRect.size == PaneLayoutSize(width: 1_024, height: 576))
  #expect(presentation.scale == 3.2)
}

@Test func explicitCompanionUsesSideBelowThenTabsWithoutChangingCanonicalOrder() throws {
  let companion = PaneLayoutCompanion(writingPaneId: "writing", agentPaneId: "agent")
  let order = ["agent", "writing"]
  let sideThreshold = 317.0 + 18.0 + 340.0
  let belowThreshold = 394.0 + 18.0 + 355.0

  let side = try writingPolicy.companionLayout(
    WritingCompanionRequest(
      companion: companion, canonicalPaneIds: order, writingKind: .text,
      writingFont: proseMetric,
      viewport: PaneLayoutSize(width: sideThreshold, height: 500),
      selectedPaneId: "writing"))
  let below = try writingPolicy.companionLayout(
    WritingCompanionRequest(
      companion: companion, canonicalPaneIds: order, writingKind: .text,
      writingFont: proseMetric,
      viewport: PaneLayoutSize(width: sideThreshold - 1, height: belowThreshold)))
  let tabs = try writingPolicy.companionLayout(
    WritingCompanionRequest(
      companion: companion, canonicalPaneIds: order, writingKind: .text,
      writingFont: proseMetric,
      viewport: PaneLayoutSize(width: sideThreshold - 1, height: belowThreshold - 1)))

  #expect(side.presentation == .sideBySide)
  #expect(side.tabOrder == order)
  #expect(side.selectedPaneId == "writing")
  #expect(side.contentRect.size == side.contentSize)
  #expect(side.tabStripRect == nil)
  #expect(below.presentation == .below)
  #expect(below.contentRect.size == below.contentSize)
  #expect(below.tabStripRect == nil)
  #expect(tabs.presentation == .tabs)
  #expect(tabs.tabStripRect?.size.height == 36)
  #expect(tabs.contentRect.origin.y == 36)
  #expect(tabs.contentRect.size.height == tabs.contentSize.height - 36)
  #expect(tabs.selectedPaneId == "agent")
}

@Test func companionRejectsInferenceAndMissingFloors() {
  let companion = PaneLayoutCompanion(writingPaneId: "writing", agentPaneId: "agent")
  #expect(throws: WritingLayoutError.invalidCompanionChildren) {
    try writingPolicy.companionLayout(
      WritingCompanionRequest(
        companion: companion, canonicalPaneIds: ["writing", "other"],
        writingKind: .text, writingFont: proseMetric,
        viewport: PaneLayoutSize(width: 1_000, height: 800)))
  }
  #expect(throws: WritingLayoutError.unsupportedKind(.terminal)) {
    try writingPolicy.companionLayout(
      WritingCompanionRequest(
        companion: companion, canonicalPaneIds: ["writing", "agent"],
        writingKind: .terminal, writingFont: proseMetric,
        viewport: PaneLayoutSize(width: 1_000, height: 800)))
  }
}

@Test func fullWidthIsAClosedTextOnlyPolicy() {
  #expect(throws: WritingLayoutError.fullWidthRequiresText) {
    try writingPolicy.layout(
      WritingLayoutRequest(
        kind: .screenplay, viewport: writingFloors[.screenplay]!,
        font: screenplayMetric, fullWidth: true))
  }
  #expect(throws: WritingLayoutError.fullWidthRequiresText) {
    try writingPolicy.layout(
      WritingLayoutRequest(
        kind: .presentation, viewport: writingFloors[.presentation]!,
        font: proseMetric, fullWidth: true))
  }
}

@Test func requestedViewportMatrixPreservesEveryWritingInvariant() throws {
  let widths = [375.0, 390, 430, 768, 1_024, 1_440, 3_440]
  let heights = [812.0, 768, 736, 672, 640, 568, 480, 430, 390, 375]
  let proseFonts = [
    PaneLayoutFontMetric(characterWidth: 11.037_048_339_843_75, lineHeight: 32, pointSize: 20),
    PaneLayoutFontMetric(characterWidth: 10, lineHeight: 32, pointSize: 20),
    PaneLayoutFontMetric(characterWidth: 12, lineHeight: 32, pointSize: 20),
  ]

  for width in widths {
    for height in heights {
      let viewport = PaneLayoutSize(width: width, height: height)
      for font in proseFonts {
        let plan = try writingPolicy.layout(
          WritingLayoutRequest(
            kind: .text, viewport: viewport, font: font, reviewVisible: true))
        guard case .text(let text) = plan.surface else {
          Issue.record("Expected text plan")
          continue
        }
        #expect(text.visibleCharacters >= 24)
        #expect(text.visibleCharacters <= 66)
        #expect(text.columnRect.size.width >= 24 * font.characterWidth)
      }

      let screenplayPlan = try writingPolicy.layout(
        WritingLayoutRequest(
          kind: .screenplay, viewport: viewport, font: screenplayMetric))
      guard case .screenplay(let screenplay) = screenplayPlan.surface else {
        Issue.record("Expected screenplay plan")
        continue
      }
      #expect(screenplay.scale >= WritingLayoutMetrics.standard.screenplayMinimumScale)
      #expect(screenplay.scale <= 1)
      let pageRatio = screenplay.pageRect.size.width / screenplay.pageRect.size.height
      #expect(abs(pageRatio - 8.5 / 11.0) < 0.000_001)

      let presentationPlan = try writingPolicy.layout(
        WritingLayoutRequest(
          kind: .presentation, viewport: viewport, font: proseMetric))
      guard case .presentation(let presentation) = presentationPlan.surface else {
        Issue.record("Expected presentation plan")
        continue
      }
      let stageRatio = presentation.stageRect.size.width / presentation.stageRect.size.height
      #expect(abs(stageRatio - 16.0 / 9.0) < 0.000_001)
      #expect(presentation.stageRect.size.width >= 320)
      #expect(presentation.stageRect.size.width <= 1_024)
    }
  }
}
