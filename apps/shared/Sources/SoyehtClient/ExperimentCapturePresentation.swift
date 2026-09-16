import Foundation
import SoyehtProtocol

public enum ExperimentCapturePresentation {
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
