import Testing

@testable import SoyehtClient

@Test func returnInTheNewDocumentSheetCreatesOnlyWithATitleAndOnlyOnce() {
  var draft = WritingDocumentDraft()
  #expect(draft.submission(creating: false) == nil)
  draft.title = "  \n "
  #expect(draft.submission(creating: false) == nil)

  draft.title = "  qa essay "
  #expect(
    draft.submission(creating: false)
      == WritingDocumentDraft.Submission(title: "qa essay", relativeFolder: nil))
  // Return again while the engine is still answering must not create a second document.
  #expect(draft.submission(creating: true) == nil)

  draft.folder = " drafts/2026 "
  #expect(draft.submission(creating: false)?.relativeFolder == "drafts/2026")
  draft.folder = "   "
  #expect(draft.submission(creating: false)?.relativeFolder == nil)
}
