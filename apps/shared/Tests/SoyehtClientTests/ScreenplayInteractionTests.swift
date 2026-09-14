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
}

@Test func screenplayVariantCyclingWrapsWithoutChangingTheWriterSelection() {
  let variants = screenplayStructure().variants[0]
  #expect(ScreenplayInteraction.adjacentVariant(in: variants, delta: 1)?.variantId == "v3")
  #expect(ScreenplayInteraction.adjacentVariant(in: variants, delta: -1)?.variantId == "v1")
  #expect(variants.selectedIndex == 1)
}

private func screenplayStructure() -> WritingStructureResult {
  let document = WritingDocument(
    file: FileDocument(
      fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1",
      path: "Screenplays/Pilot.fountain", language: .plain_text, revision: "4",
      diskRevision: "4", lineCount: 6, byteCount: "90", dirty: false,
      conflicted: false, watching: true),
    kind: .screenplay, section: .screenplays, sidecarRevision: "2",
    font: .sans, focusMode: false)
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
    ], beats: [], truncated: false)
}

private let author = FileAuthor(
  actor: Actor(role: .human_local, client: "mac", paneId: "agent-1"), name: "aria")
