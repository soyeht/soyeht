import Foundation
import SoyehtProtocol

extension EngineClient {
  public func setExperimentCapture(
    paneId: String, on: Bool
  ) async throws -> PaneExperimentSetResult {
    try await mutation(
      .paneExperimentSet,
      PaneExperimentSetParams(
        paneId: paneId, on: on, idempotencyKey: UUID().uuidString)
    ).decoded(PaneExperimentSetResult.self)
  }
  public func deleteExperimentCaptureData(
    paneId: String
  ) async throws -> PaneExperimentDeleteResult {
    try await mutation(
      .paneExperimentDelete,
      PaneExperimentDeleteParams(paneId: paneId, idempotencyKey: UUID().uuidString)
    ).decoded(PaneExperimentDeleteResult.self)
  }
}
