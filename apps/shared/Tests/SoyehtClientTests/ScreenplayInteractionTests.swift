import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func screenplayAdvanceAndFormattingStayClosedAndFountainValid() {
  #expect(ScreenplayInteraction.nextKind(after: .scene_heading, tab: false) == .action)
  #expect(ScreenplayInteraction.nextKind(after: .action, tab: true) == .character)
  #expect(ScreenplayInteraction.nextKind(after: .character, tab: false) == .dialogue)
  #expect(ScreenplayInteraction.nextKind(after: .dialogue, tab: true) == .parenthetical)
  #expect(ScreenplayInteraction.nextKind(after: .dialogue, tab: false) == .character)
  #expect(ScreenplayInteraction.nextKind(after: .transition, tab: false) == .scene_heading)

  #expect(
    ScreenplayInteraction.formattedLine("Kitchen - day", as: .scene_heading)
      == ".KITCHEN - DAY")
  #expect(ScreenplayInteraction.formattedLine("ana", as: .character) == "@ANA")
  #expect(ScreenplayInteraction.formattedLine("quietly", as: .parenthetical) == "(quietly)")
  #expect(ScreenplayInteraction.formattedLine("cut to:", as: .transition) == ">CUT TO:")
  #expect(ScreenplayInteraction.formattedLine("close up", as: .shot) == "CLOSE UP:")
}

@Test func screenplayEnterThenTabCreatesASecondElementWithoutRewritingTheFirst() throws {
  let heading = "INT. COFFEE SHOP - DAY"
  let enter = try #require(
    ScreenplayInteraction.advanceEdit(
      source: heading, cursor: 4, kind: .scene_heading, tab: false))
  var source = (heading as NSString).replacingCharacters(in: enter.range, with: enter.replacement)
  #expect(source == "INT. COFFEE SHOP - DAY\n\n")

  let tab = try #require(
    ScreenplayInteraction.advanceEdit(
      source: source, cursor: source.utf16.count, kind: .action, tab: true))
  source = (source as NSString).replacingCharacters(in: tab.range, with: tab.replacement)
  #expect(source == "INT. COFFEE SHOP - DAY\n\n@")
  #expect(source + "BOB" == "INT. COFFEE SHOP - DAY\n\n@BOB")
}

@Test func screenplaySustainedTypingKeepsEveryCharacterAndWireEditInOrder() throws {
  let paragraph =
    "The rain rattles against the windows while every customer turns toward the door."
  var source = ""
  var cursor = 0
  var wireText = ""
  for page in 0..<2 {
    for line in 0..<55 {
      let value = String(page + 1) + "." + String(line + 1) + " " + paragraph
      for character in value + "\n" {
        let edit = try #require(
          ScreenplayInteraction.typingEdit(
            source: source, firstLine: 0, range: NSRange(location: cursor, length: 0),
            replacement: String(character)))
        source = edit.source
        cursor = edit.cursor
        wireText = apply(edit.wireEdit, to: wireText)
      }
    }
  }
  #expect(source == wireText)
  #expect(source.utf8.count > 9_000)
  #expect(source.split(separator: "\n").count == 110)
  #expect(cursor == source.utf16.count)
}

@Test func screenplaySmartTypeUsesOnlyKnownBoundedFacts() {
  let structure = screenplayStructure()
  #expect(ScreenplayInteraction.suggestions(for: .character, prefix: "a", in: structure) == ["ANA"])
  #expect(
    ScreenplayInteraction.suggestions(for: .scene_heading, prefix: "kit", in: structure)
      == ["KITCHEN"])
  #expect(
    ScreenplayInteraction.suggestions(for: .transition, prefix: "cut", in: structure)
      == ["CUT TO:"])
  #expect(
    ScreenplayInteraction.suggestions(for: .parenthetical, prefix: "(v", in: structure)
      == ["(V.O.)"])
  #expect(ScreenplayInteraction.suggestionKind(at: 2, in: structure) == .character)
  #expect(ScreenplayInteraction.suggestionKind(at: 3, in: structure) == .dialogue)
  #expect(
    ScreenplayInteraction.suggestionKind(at: 0, in: emptyScreenplayStructure()) == .scene_heading)
}

@Test func screenplayEditRequestNeverCrossesAFileOrReloadedViewport() {
  #expect(
    ScreenplayInteraction.editRequestMatchesViewport(
      requestFileId: "file-1", requestRevision: "4",
      viewportFileId: "file-1", viewportRevision: "4"))
  #expect(
    !ScreenplayInteraction.editRequestMatchesViewport(
      requestFileId: "file-1", requestRevision: "4",
      viewportFileId: "file-1", viewportRevision: "5"))
  #expect(
    !ScreenplayInteraction.editRequestMatchesViewport(
      requestFileId: "file-1", requestRevision: "4",
      viewportFileId: "file-2", viewportRevision: "4"))
}

@Test func screenplayVariantCyclingWrapsWithoutChangingTheWriterSelection() {
  let variants = screenplayStructure().variants[0]
  #expect(ScreenplayInteraction.adjacentVariant(in: variants, delta: 1)?.variantId == "v3")
  #expect(ScreenplayInteraction.adjacentVariant(in: variants, delta: -1)?.variantId == "v1")
  #expect(variants.selectedIndex == 1)
}

@Test func screenplayProjectionHidesOnlySourceMarkersAndKeepsOpeningTransitionLeading() {
  #expect(
    ScreenplayInteraction.lineProjection(source: ".INT. ROOM - DAY", kind: .scene_heading)
      == ScreenplayLineProjection(
        hiddenRanges: [NSRange(location: 0, length: 1)], openingTransition: false))
  #expect(
    ScreenplayInteraction.lineProjection(source: "@ANA:", kind: .character)
      == ScreenplayLineProjection(
        hiddenRanges: [NSRange(location: 0, length: 1), NSRange(location: 4, length: 1)],
        openingTransition: false))
  #expect(
    ScreenplayInteraction.lineProjection(source: ">FADE IN:", kind: .transition)
      == ScreenplayLineProjection(
        hiddenRanges: [NSRange(location: 0, length: 1)], openingTransition: true))
  #expect(
    ScreenplayInteraction.lineProjection(source: ">CUT TO:>", kind: .transition)
      == ScreenplayLineProjection(
        hiddenRanges: [NSRange(location: 0, length: 1), NSRange(location: 8, length: 1)],
        openingTransition: false))
  #expect(
    ScreenplayInteraction.lineProjection(source: "[action] Bob sits.", kind: .action)
      == ScreenplayLineProjection(
        hiddenRanges: [NSRange(location: 0, length: 9)], openingTransition: false))
}

private func screenplayStructure() -> WritingStructureResult {
  let document = WritingDocument(
    file: FileDocument(
      fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1",
      path: "Screenplays/Pilot.fountain", language: .plain_text, revision: "4",
      diskRevision: "4", lineCount: 6, byteCount: "90", dirty: false,
      conflicted: false, watching: true),
    kind: .screenplay, section: .screenplays, sidecarRevision: "2",
    font: .sans, focusMode: false, fullWidth: false)
  return WritingStructureResult(
    document: document, slides: [],
    scenes: [
      ScreenplayScene(
        sceneId: "scene-1", ordinal: 0, heading: "INT. KITCHEN - DAY", line: 0,
        linkedBeatIds: [])
    ],
    elements: [
      ScreenplayElement(
        elementId: "e1", kind: .scene_heading, startLine: 0, endLine: 0,
        text: "INT. KITCHEN - DAY", sceneId: "scene-1"),
      ScreenplayElement(
        elementId: "e2", kind: .character, startLine: 2, endLine: 2,
        text: "ANA", sceneId: "scene-1"),
    ],
    variants: [
      WritingLineVariants(
        lineId: "line-3", line: 3,
        alternates: [
          WritingVariant(variantId: "v1", text: "One", author: author),
          WritingVariant(variantId: "v2", text: "Two", author: author),
          WritingVariant(variantId: "v3", text: "Three", author: author),
        ], selectedIndex: 1)
    ], beats: [], wordCount: 13, truncated: false)
}

private func emptyScreenplayStructure() -> WritingStructureResult {
  let value = screenplayStructure()
  return WritingStructureResult(
    document: value.document, slides: [], scenes: [], elements: [], variants: [], beats: [],
    wordCount: 0, truncated: false)
}

private let author = FileAuthor(
  actor: Actor(role: .human_local, client: "mac", paneId: "agent-1"), name: "aria")

private func apply(_ edit: TextEdit, to source: String) -> String {
  let start = utf16Offset(edit.range.start, in: source)
  let end = utf16Offset(edit.range.end, in: source)
  return (source as NSString).replacingCharacters(
    in: NSRange(location: start, length: end - start), with: edit.text)
}

private func utf16Offset(_ position: TextPosition, in source: String) -> Int {
  let lines = source.components(separatedBy: "\n")
  let line = min(Int(position.line), max(0, lines.count - 1))
  return lines.prefix(line).reduce(0) { $0 + $1.utf16.count + 1 } + Int(position.column)
}
