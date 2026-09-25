import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func newFilePermissionShowsAddedLines() {
  let change = PermissionFileChange(path: "/project/note.txt", newText: "first\nsecond\n")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.removed.isEmpty)
  #expect(preview.added == ["first", "second"])
  #expect(!preview.unavailable)
}

@Test func editedFilePermissionShowsOnlyChangedLines() {
  let change = PermissionFileChange(
    path: "/project/note.txt", oldText: "same\nbefore\nend\n",
    newText: "same\nafter\nend\n")
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.removed == ["before"])
  #expect(preview.added == ["after"])
}

@Test func veryLongPermissionDiffFailsClosed() {
  let change = PermissionFileChange(
    path: "/project/large.txt", newText: String(repeating: "line\n", count: 401))
  let preview = PermissionPreviewPolicy.preview(change)
  #expect(preview.unavailable)
  #expect(preview.added.isEmpty)
}
