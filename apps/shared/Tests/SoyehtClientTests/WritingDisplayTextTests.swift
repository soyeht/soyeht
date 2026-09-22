import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func writingCountsUseOneSingularAndPluralSource() {
  #expect(WritingDisplayText.count(0, kind: .word) == "0 words")
  #expect(WritingDisplayText.count(1, kind: .word) == "1 word")
  #expect(WritingDisplayText.count(2, kind: .scene) == "2 scenes")
  #expect(WritingDisplayText.count(1, kind: .slide) == "1 slide")
}

@Test func writingNamesStaySharedAcrossMacAndPhone() {
  #expect(WritingDisplayText.fontName(.reading_serif) == "Newsreader")
  #expect(WritingDisplayText.fontName(.classic_serif) == "Classic serif")
  #expect(WritingDisplayText.fontName(.sans) == "Sans")
  #expect(WritingDisplayText.slideTitle("") == "Untitled slide")
  #expect(WritingDisplayText.slideTitle("Opening") == "Opening")
  #expect(
    WritingDisplayText.screenplayElementKinds == [
      .scene_heading, .action, .character, .parenthetical, .dialogue, .transition, .shot,
    ])
  #expect(WritingDisplayText.screenplayElementName(.scene_heading) == "Scene heading")
  #expect(WritingDisplayText.screenplayElementName(.dialogue) == "Dialogue")
}
