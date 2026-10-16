import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

#if canImport(AppKit)
  import AppKit
#endif

@Test func everyScreenplayElementHasItsPageStyle() {
  func style(_ kind: ScreenplayElementKind, _ line: String = "TEXT") -> ScreenplayLineStyle {
    ScreenplayLineStyle(kind: kind, line: line)
  }
  #expect(style(.scene_heading).bold)
  #expect(style(.shot).bold)
  #expect(!style(.action).bold)
  #expect(style(.action).alignment == .natural)
  #expect(style(.character).alignment == .center)
  #expect(style(.character).bold)
  #expect(style(.parenthetical).alignment == .center)
  #expect(style(.parenthetical).secondary)
  let dialogue = style(.dialogue)
  #expect(Double(dialogue.headIndent) == ScreenplayLineStyle.dialogueInset)
  #expect(Double(dialogue.tailIndent) == -ScreenplayLineStyle.dialogueInset)
  #expect(!dialogue.secondary)
}

@Test func aTransitionAlignsRightExceptTheOpeningOne() {
  #expect(ScreenplayLineStyle(kind: .transition, line: ">CUT TO:").alignment == .right)
  #expect(ScreenplayLineStyle(kind: .transition, line: ">FADE IN:").alignment == .left)
  #expect(ScreenplayLineStyle(kind: .transition, line: ">CUT TO:").secondary)
}

@Test func theMarkersOfAForcedLineStayHiddenInTheStyle() {
  let heading = ScreenplayLineStyle(kind: .scene_heading, line: ".INT. SHOP")
  #expect(heading.hiddenRanges == [NSRange(location: 0, length: 1)])
  #expect(ScreenplayLineStyle(kind: .scene_heading, line: "INT. SHOP").hiddenRanges.isEmpty)
}

@Test func aLineStyleBecomesTheParagraphOfThePage() {
  let base = ScreenplayLineStyle.basePageParagraph()
  #expect(Double(base.minimumLineHeight) == PaneLayoutContentMetrics.screenplayLineHeight)
  #expect(Double(base.maximumLineHeight) == PaneLayoutContentMetrics.screenplayLineHeight)
  #expect(Double(base.paragraphSpacing) == ScreenplayLineStyle.paragraphSpacing)
  #expect(base.alignment == .natural)

  let dialogue = ScreenplayLineStyle(kind: .dialogue, line: "Hello.").paragraphStyle()
  #expect(Double(dialogue.firstLineHeadIndent) == ScreenplayLineStyle.dialogueInset)
  #expect(Double(dialogue.headIndent) == ScreenplayLineStyle.dialogueInset)
  #expect(Double(dialogue.tailIndent) == -ScreenplayLineStyle.dialogueInset)
  #expect(ScreenplayLineStyle(kind: .character, line: "@ANA").paragraphStyle().alignment == .center)
  #expect(
    ScreenplayLineStyle(kind: .transition, line: ">CUT TO:").paragraphStyle().alignment == .right)
}

@Test func hiddenMarkersAreRangesOfTheWholeTextInsideTheirLine() {
  let style = ScreenplayLineStyle(kind: .character, line: "@ANA:")
  #expect(style.hiddenRanges == [NSRange(location: 0, length: 1), NSRange(location: 4, length: 1)])
  let line = NSRange(location: 10, length: 5)
  #expect(
    style.hiddenMarkerRanges(inLine: line) == [
      NSRange(location: 10, length: 1), NSRange(location: 14, length: 1),
    ])
  #expect(
    style.hiddenMarkerRanges(inLine: NSRange(location: 10, length: 2)) == [
      NSRange(location: 10, length: 1)
    ])
}

@Test func aLineStyleIsAppliedWithTheAppearanceOfThePlatform() {
  let appearance = ScreenplayPageAppearance(
    boldFont: "bold", secondaryInk: "secondary", hiddenMarkerFont: "hidden",
    hiddenMarkerInk: "face")
  let text = NSMutableAttributedString(string: "xx@ANA:xx")
  let range = NSRange(location: 2, length: 5)
  ScreenplayLineStyle(kind: .character, line: "@ANA:").apply(
    to: text, range: range, appearance: appearance)
  #expect(text.attribute(.font, at: 3, effectiveRange: nil) as? String == "bold")
  #expect(text.attribute(.font, at: 2, effectiveRange: nil) as? String == "hidden")
  #expect(text.attribute(.font, at: 6, effectiveRange: nil) as? String == "hidden")
  #expect(text.attribute(.foregroundColor, at: 2, effectiveRange: nil) as? String == "face")
  #expect(text.attribute(.foregroundColor, at: 4, effectiveRange: nil) == nil)
  #expect(text.attribute(.font, at: 0, effectiveRange: nil) == nil)
  let paragraph = text.attribute(.paragraphStyle, at: 4, effectiveRange: nil) as? NSParagraphStyle
  #expect(paragraph?.alignment == .center)

  let dimmed = NSMutableAttributedString(string: "(softly)")
  ScreenplayLineStyle(kind: .parenthetical, line: "(softly)").apply(
    to: dimmed, range: NSRange(location: 0, length: 8), appearance: appearance)
  #expect(dimmed.attribute(.foregroundColor, at: 1, effectiveRange: nil) as? String == "secondary")
  #expect(dimmed.attribute(.font, at: 1, effectiveRange: nil) == nil)
}
