import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func editorDiffAlignsChangeRunsAndProjectsGutterAndBlameFacts() throws {
  let result = FileDiffResult(
    fileId: "file-1", revision: "7",
    hunks: [
      DiffHunk(
        hunkId: "h1", oldStart: 2, oldLines: 5, newStart: 2, newLines: 5,
        lines: [
          line(.context, old: 2, new: 2, "before"),
          line(.removed, old: 3, "old value"),
          line(.added, new: 3, "new value"),
          line(.added, new: 4, "inserted"),
          line(.context, old: 4, new: 5, "after"),
          line(.removed, old: 5, "deleted one"),
          line(.removed, old: 6, "deleted two"),
        ])
    ],
    blame: [FileBlame(line: 2, commit: "abc123", author: "Sam", summary: "Keep context")])

  let presentation = EditorDiffPresentation(result)

  #expect(presentation.rows.count == 6)
  #expect(
    presentation.rows.map(\.kind) == [.context, .modified, .added, .context, .removed, .removed])
  #expect(presentation.rows[1].old.line == 3)
  #expect(presentation.rows[1].new.line == 3)
  #expect(presentation.rows[2].old.text == nil)
  #expect(presentation.rows[2].new.text == "inserted")
  #expect(presentation.markers[3] == [.modified])
  #expect(presentation.markers[4] == [.added])
  #expect(presentation.markers[6] == [.deleted])
  #expect(presentation.gutterMarkers(lineCount: 6)[5] == [.deleted])
  #expect(presentation.blameByLine[2]?.author == "Sam")
}

private func line(
  _ kind: DiffLineKind, old: UInt32? = nil, new: UInt32? = nil, _ text: String
) -> DiffLine {
  DiffLine(kind: kind, oldLine: old, newLine: new, text: text)
}
