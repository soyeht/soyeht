import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func vimIsNormalInitiallyAndInsertEscapeRestoresNormal() {
  var vim = EditorVimMachine()
  #expect(vim.mode == .normal)
  let insert = vim.handle(
    .character("i"), text: "one", viewportStartLine: 0,
    selections: [NSRange(location: 0, length: 0)], language: .rust)
  #expect(insert.handled)
  #expect(vim.mode == .insert)
  #expect(
    vim.handle(
      .character("x"), text: "one", viewportStartLine: 0,
      selections: [NSRange(location: 0, length: 0)], language: .rust
    ).handled == false)
  _ = vim.handle(
    .escape, text: "one", viewportStartLine: 0,
    selections: [NSRange(location: 1, length: 0)], language: .rust)
  #expect(vim.mode == .normal)
}

@Test func vimMotionsAndOperatorsProduceUtf16SafePlans() throws {
  var vim = EditorVimMachine()
  let text = "one 😀 two\nthree"
  let word = vim.handle(
    .character("w"), text: text, viewportStartLine: 7,
    selections: [NSRange(location: 0, length: 0)], language: .rust)
  #expect(word.selections == [NSRange(location: 7, length: 0)])

  _ = vim.handle(
    .character("d"), text: text, viewportStartLine: 7,
    selections: word.selections ?? [], language: .rust)
  let deleted = vim.handle(
    .character("w"), text: text, viewportStartLine: 7,
    selections: word.selections ?? [], language: .rust)
  let plan = try #require(deleted.plan)
  #expect(EditorEditing.applying(plan.replacements, to: text) == "one 😀 three")
  #expect(plan.edits.first?.range.start.line == 7)
  #expect(plan.edits.first?.range.start.column == 7)

  _ = vim.handle(
    .character("d"), text: "first\nsecond", viewportStartLine: 0,
    selections: [NSRange(location: 0, length: 0)], language: .rust)
  let line = try #require(
    vim.handle(
      .character("d"), text: "first\nsecond", viewportStartLine: 0,
      selections: [NSRange(location: 0, length: 0)], language: .rust
    ).plan)
  #expect(EditorEditing.applying(line.replacements, to: "first\nsecond") == "second")
}

@Test func vimVisualYankAndPasteKeepTheOtherText() throws {
  var vim = EditorVimMachine()
  _ = vim.handle(
    .character("v"), text: "abc xyz", viewportStartLine: 0,
    selections: [NSRange(location: 0, length: 0)], language: .plain_text)
  var selection = [NSRange(location: 0, length: 1)]
  for _ in 0..<2 {
    let moved = vim.handle(
      .character("l"), text: "abc xyz", viewportStartLine: 0,
      selections: selection, language: .plain_text)
    selection = moved.selections ?? selection
  }
  _ = vim.handle(
    .character("y"), text: "abc xyz", viewportStartLine: 0,
    selections: selection, language: .plain_text)
  let pasted = try #require(
    vim.handle(
      .character("p"), text: "abc xyz", viewportStartLine: 0,
      selections: [NSRange(location: 6, length: 0)], language: .plain_text
    ).plan)
  #expect(EditorEditing.applying(pasted.replacements, to: "abc xyz") == "abc xyzabc")
}

@Test func vimOnlyAcceptsTheFourBoundedExCommands() {
  let cases: [(String, EditorVimEffect?)] = [
    (":w", .save), (":q", .close), (":e", .reload),
    (":%s/old/new/g", .substitute(pattern: "old", replacement: "new", everyMatch: true)),
    (":%s/a\\/b/c/", .substitute(pattern: "a/b", replacement: "c", everyMatch: false)),
    (":wq", nil), (":!rm", nil), (":set", nil),
  ]
  for (command, expected) in cases {
    var vim = EditorVimMachine()
    for character in command {
      _ = vim.handle(
        .character(String(character)), text: "", viewportStartLine: 0,
        selections: [NSRange(location: 0, length: 0)], language: .plain_text)
    }
    let result = vim.handle(
      .enter, text: "", viewportStartLine: 0,
      selections: [NSRange(location: 0, length: 0)], language: .plain_text)
    #expect(result.effect == expected)
  }
}

@Test func vimSubstitutionBuildsAbsoluteUtf16EditsAndHonorsTheGlobalFlag() throws {
  let lines = [
    EditorLine(line: 40, text: "😀 old old", highlights: []),
    EditorLine(line: 41, text: "old", highlights: []),
  ]
  let first = try #require(
    EditorVimSubstitution.edits(
      lines: lines, pattern: "old", replacement: "new", everyMatch: false,
      remainingLimit: 10))
  #expect(first.count == 2)
  #expect(first[0].range.start.line == 40)
  #expect(first[0].range.start.column == 3)
  let every = try #require(
    EditorVimSubstitution.edits(
      lines: lines, pattern: "old", replacement: "new", everyMatch: true,
      remainingLimit: 10))
  #expect(every.count == 3)
  #expect(
    EditorVimSubstitution.edits(
      lines: lines, pattern: "old", replacement: "new", everyMatch: true,
      remainingLimit: 2) == nil)
}

@Test @MainActor func vimPreferenceIsOffByDefaultAndPersistsExplicitChanges() {
  let preferences = VimMemoryPreferences()
  let initial = EditorInputPreferences(preferences: preferences)
  #expect(initial.vimEnabled == false)
  initial.setVimEnabled(true)
  #expect(EditorInputPreferences(preferences: preferences).vimEnabled)
}

private final class VimMemoryPreferences: ClientPreferenceStore {
  private var values: [String: Bool] = [:]
  func bool(forKey key: String) -> Bool { values[key] ?? false }
  func set(_ value: Bool, forKey key: String) { values[key] = value }
}
