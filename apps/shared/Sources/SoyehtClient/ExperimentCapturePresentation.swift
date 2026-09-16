import Foundation
import SoyehtProtocol

public enum ExperimentCapturePresentation {
  public static func deletionText(_ result: PaneExperimentDeleteResult) -> String {
    guard result.files > 0 else {
      return result.nothingRemained ? "No captured data remained." : "No data was deleted."
    }
    let bytes = Int64(result.bytes) ?? 0
    let size = ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    return "Deleted \(result.files) files · \(size)."
  }
}
