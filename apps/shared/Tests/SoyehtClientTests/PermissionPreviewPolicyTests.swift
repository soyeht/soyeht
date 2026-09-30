import SoyehtProtocol
import SwiftUI
import Testing

@testable import SoyehtClient

@Test func newFilePermissionShowsAddedLines() {
  let change = PermissionFileChange(path: "/project/note.txt", newText: "first\nsecond\n")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.rows.map(\.kind) == [.added, .added])
  #expect(preview.rows.map(\.text) == ["first", "second"])
  #expect(preview.rows.map(\.id) == [0, 1])
  #expect(!preview.unavailable)
}

@Test func editedFilePermissionShowsOnlyChangedLines() {
  let change = PermissionFileChange(
    path: "/project/note.txt", oldText: "same\nbefore\nend\n",
    newText: "same\nafter\nend\n")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.rows.map(\.kind) == [.removed, .added])
  #expect(preview.rows.map(\.text) == ["before", "after"])
}

@Test func claudeEditPermissionProducesBothRows() {
  // Sanitized payload captured from claude-acp's Edit permission request.
  let change = PermissionFileChange(
    path: "/project/note.txt", oldText: "hello", newText: "goodbye")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(!preview.unavailable)
  #expect(preview.rows.map(\.kind) == [.removed, .added])
  #expect(preview.rows.map(\.text) == ["hello", "goodbye"])
  #expect(preview.rows.map(\.id) == [0, 1])
}

@Test func multiLineEditPermissionKeepsUniqueRowIDs() {
  let change = PermissionFileChange(
    path: "/project/note.txt", oldText: "one\ntwo\nthree", newText: "red\nblue\ngreen")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.rows.map(\.kind) == [.removed, .removed, .removed, .added, .added, .added])
  #expect(preview.rows.map(\.text) == ["one", "two", "three", "red", "blue", "green"])
  #expect(preview.rows.map(\.id) == [0, 1, 2, 3, 4, 5])
}

@Test func veryLongPermissionDiffFailsClosed() {
  let change = PermissionFileChange(
    path: "/project/large.txt",
    newText: String(repeating: "line\n", count: ProtocolLimits.permissionPreviewLines + 1))
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.unavailable)
  #expect(preview.rows.isEmpty)
}

@Test func permissionWellsSizeToTheirVisibleLines() {
  #expect(PermissionPreviewPolicy.lineCount("curl -sI https://example.com | head -1") == 1)
  #expect(PermissionPreviewPolicy.lineCount("HTTP/2 200 \r\n") == 2)
  #expect(PermissionPreviewPolicy.visibleLineSpacer(lines: 0) == " ")
  #expect(PermissionPreviewPolicy.visibleLineSpacer(lines: 2) == " \n ")
  #expect(
    PermissionPreviewPolicy.lineCount(PermissionPreviewPolicy.visibleLineSpacer(lines: 500))
      == PermissionPreviewPolicy.visibleLineLimit)
}

#if os(macOS)
  /// Height a view keeps when its container offers it nothing, as in a short stacked pane.
  @MainActor private func minimumHeight(_ view: some View) -> CGFloat {
    NSHostingController(rootView: view).sizeThatFits(in: CGSize(width: 320, height: 0)).height
  }

  @MainActor private func textBoxHeight(label: String, text: String) -> CGFloat {
    let style = AgentConversationStyle.standard
    let box = VStack(alignment: .leading, spacing: 6) {
      Text(label).font(style.captionFont)
      Text(text).font(style.monoSmallFont).fixedSize().padding(8)
    }
    return NSHostingController(rootView: box)
      .sizeThatFits(in: CGSize(width: 320, height: 10_000)).height
  }

  @MainActor @Test func codexCommandStaysVisibleWhenThePaneIsCrowded() {
    // Presentation stored for a real Codex exec approval whose Mac card showed an empty box:
    // the scrolling command well had no minimum height and collapsed to its padding.
    let command = "curl -sI https://example.com | head -1"
    let presentation = PermissionPresentation(
      command: command, fileChanges: [], previewOmitted: false)
    let view = PermissionPresentationView(presentation: presentation, style: .standard)
    #expect(minimumHeight(view) >= textBoxHeight(label: "Command", text: command) - 0.5)
  }

  @MainActor @Test func toolArgumentsStayVisibleWhenThePaneIsCrowded() {
    let arguments = [
      PermissionToolArgument(name: "targetPaneId", value: "Claude"),
      PermissionToolArgument(name: "text", value: "HTTP/2 200 \r\n"),
    ]
    let presentation = PermissionPresentation(
      toolArguments: arguments, fileChanges: [], previewOmitted: false)
    let view = PermissionPresentationView(presentation: presentation, style: .standard)
    let expected = arguments.map { textBoxHeight(label: $0.name, text: $0.value) }
      .reduce(0, +)
    #expect(minimumHeight(view) >= expected - 0.5)
  }
#endif
