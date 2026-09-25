import SoyehtProtocol
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
