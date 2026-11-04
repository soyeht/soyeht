import Testing

@testable import SoyehtClient

/// The composer field forgets the caret of a sent prompt: sending clears the text while the field
/// still holds the old caret, and SwiftUI stops the app on an index past the end of the text.
@MainActor struct AgentComposerCaretTests {
  @Test func aClearedPromptForgetsTheCaretOfTheSentText() {
    guard #available(macOS 15, iOS 18, *) else { return }
    let caret = AgentComposerCaret()
    caret.place(at: 11, in: "hello world")
    #expect(caret.selection(in: "") == nil)
    #expect(caret.selection(in: "h") == nil)
  }

  @Test func aCaretInsideTheTextIsKept() throws {
    guard #available(macOS 15, iOS 18, *) else { return }
    let caret = AgentComposerCaret()
    let text = "hello world"
    caret.place(at: 5, in: text)
    #expect(caret.offset(in: text) == 5)
    let selection = try #require(caret.selection(in: text))
    guard case .selection(let range) = selection.indices else {
      Issue.record("expected one range")
      return
    }
    #expect(text.distance(from: text.startIndex, to: range.upperBound) == 5)
  }

  @Test func noCaretReadsAsNone() {
    guard #available(macOS 15, iOS 18, *) else { return }
    #expect(AgentComposerCaret().selection(in: "text") == nil)
  }
}
