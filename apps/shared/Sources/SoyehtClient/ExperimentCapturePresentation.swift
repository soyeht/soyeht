import Foundation
import SoyehtProtocol

public enum ExperimentCapturePresentation {
  public static func consentTitle(for mode: PaneMode) -> String {
    mode == .agent ? "Record agent state labels?" : "Record raw terminal observations?"
  }

  public static func consentMessage(for mode: PaneMode) -> String {
    if mode == .agent {
      return
        "This Dev-only experiment stores timestamped agent state labels on this Mac. It does not store the conversation transcript."
    }
    return
      "This Dev-only experiment stores raw terminal text on this Mac. It can contain tokens, credentials, and private source."
  }

  public static func activeLabel(for mode: PaneMode) -> String {
    mode == .agent ? "Recording agent state labels" : "Recording terminal text"
  }

  public static func deletionText(_ result: PaneExperimentDeleteResult) -> String {
    let deletion: String
    if result.files > 0 {
      let bytes = Int64(result.bytes) ?? 0
      let size = ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
      deletion = "Deleted \(result.files) files · \(size)."
    } else {
      deletion = result.nothingRemained ? "No captured data remained." : "No data was deleted."
    }
    guard let path = result.exportsPath, !path.isEmpty else {
      return deletion
    }
    return "\(deletion) Export files were not deleted; review them at \(path)."
  }
}
