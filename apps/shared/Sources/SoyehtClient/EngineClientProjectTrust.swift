import Foundation
import SoyehtProtocol
import SwiftUI

/// The project trust domain on the wire: `EngineClient` asks, `ProjectTrustReplica` keeps the
/// answers. Only the signed Mac app or a paired device may answer; the engine refuses the rest.
extension EngineClient {
  func applyProjectTrustSnapshot(_ sections: [String: [JsonValue]]) throws {
    projectTrust.load(
      try (sections[SnapshotSection.trust.rawValue] ?? []).map {
        try $0.decoded(ProjectTrustEntry.self)
      })
  }

  func applyProjectTrustEvent(_ params: JsonValue) throws {
    projectTrust.apply(try params.decoded(ProjectTrustChangedEvent.self))
  }

  /// Answers the trust question of `workspaceId`'s folder and returns whether the engine took the
  /// answer. A refusal is shown as the policy's failure text and the sheet is not asked again in
  /// this session, so a refusal cannot loop.
  @discardableResult
  public func setProjectTrust(workspaceId: String, to choice: ProjectTrustChoice) async -> Bool {
    do {
      let result = try await mutation(
        .projectTrustSet,
        ProjectTrustSetParams(
          workspaceId: workspaceId, trust: choice, idempotencyKey: UUID().uuidString)
      ).decoded(ProjectTrustSetResult.self)
      projectTrust.apply(result)
      return true
    } catch is CancellationError {
      return false
    } catch {
      projectTrust.putOff(workspaceId)
      problem = ClientPolicy.projectTrustFailed
      return false
    }
  }

  /// Whether the "Trust this folder?" sheet is up while `selected` is the workspace on screen.
  /// Closing it any other way than an answer puts the question off until relaunch.
  public func projectTrustPrompt(selected: String?) -> Binding<Bool> {
    Binding(
      get: { self.projectTrust.promptedWorkspace(selected: selected) != nil },
      set: { presented in
        if !presented, let id = self.projectTrust.promptedWorkspace(selected: selected) {
          self.projectTrust.putOff(id)
        }
      })
  }
}
