import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func captureDeletionTextDistinguishesVerifiedAbsenceFromDeletedFiles() {
  let empty = PaneExperimentDeleteResult(files: 0, bytes: "0", nothingRemained: true)
  #expect(ExperimentCapturePresentation.deletionText(empty) == "No captured data remained.")

  let deleted = PaneExperimentDeleteResult(files: 3, bytes: "2048", nothingRemained: true)
  let receipt = ExperimentCapturePresentation.deletionText(deleted)
  #expect(receipt.hasPrefix("Deleted 3 files · "))
  #expect(receipt != ExperimentCapturePresentation.deletionText(empty))

  let exportsExcluded = PaneExperimentDeleteResult(
    files: 3, bytes: "2048", nothingRemained: true, exportsExcluded: true,
    exportsPath: "/profile/experiments/pane-state/exports")
  let exclusionReceipt = ExperimentCapturePresentation.deletionText(exportsExcluded)
  #expect(exclusionReceipt.hasPrefix("Deleted 3 files · "))
  #expect(
    exclusionReceipt.hasSuffix(
      "Export files were not deleted; review them at /profile/experiments/pane-state/exports."
    )
  )
}
