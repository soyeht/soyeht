import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func experimentCaptureNamesWhatEachPaneStores() {
  #expect(
    ExperimentCapturePresentation.consentTitle(for: .terminal)
      == "Record raw terminal observations?")
  #expect(
    ExperimentCapturePresentation.consentMessage(for: .terminal).contains("credentials"))
  #expect(
    ExperimentCapturePresentation.activeLabel(for: .terminal) == "Recording terminal text")
  #expect(
    ExperimentCapturePresentation.consentTitle(for: .agent) == "Record agent state labels?")
  #expect(
    ExperimentCapturePresentation.consentMessage(for: .agent)
      == "This Dev-only experiment stores timestamped agent state labels on this Mac. It does not store the conversation transcript."
  )
  #expect(
    ExperimentCapturePresentation.activeLabel(for: .agent) == "Recording agent state labels")
}

@Test func captureDeletionTextDistinguishesVerifiedAbsenceFromDeletedFiles() {
  let empty = PaneExperimentDeleteResult(files: 0, bytes: "0", nothingRemained: true)
  #expect(ExperimentCapturePresentation.deletionText(empty) == "No captured data remained.")

  let deleted = PaneExperimentDeleteResult(files: 3, bytes: "2048", nothingRemained: true)
  let receipt = ExperimentCapturePresentation.deletionText(deleted)
  #expect(receipt.hasPrefix("Deleted 3 files · "))
  #expect(receipt != ExperimentCapturePresentation.deletionText(empty))

  let exportsPathReceipt = PaneExperimentDeleteResult(
    files: 3, bytes: "2048", nothingRemained: true,
    exportsPath: "/profile/experiments/pane-state/exports")
  let exclusionReceipt = ExperimentCapturePresentation.deletionText(exportsPathReceipt)
  #expect(exclusionReceipt.hasPrefix("Deleted 3 files · "))
  #expect(
    exclusionReceipt.hasSuffix(
      "Export files were not deleted; review them at /profile/experiments/pane-state/exports."
    )
  )
}
