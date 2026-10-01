import Foundation
import SoyehtProtocol

extension EngineClient {
  /// Questions still waiting for a person, across every agent pane.
  public var openRequestIds: Set<String> {
    Set(
      agentPanes.values.flatMap { [$0.permission?.requestId, $0.elicitation?.requestId] }
        .compactMap { $0 })
  }

  /// Waits for the handshake and snapshot, for work started before the app is on screen,
  /// such as a notification action.
  public func waitUntilConnected(timeout: Duration = .seconds(20)) async throws {
    let deadline = ContinuousClock.now + timeout
    while !connected {
      guard ContinuousClock.now < deadline else { throw ClientFailure.timeout }
      try await Task.sleep(for: .milliseconds(100))
    }
  }

  /// Registers this device's APNs token; registering again replaces it.
  public func registerPush(token: String, environment: PushEnvironment) async throws {
    _ = try await mutation(
      "push.register",
      PushRegisterParams(
        platform: .apns, token: token, environment: environment,
        idempotencyKey: UUID().uuidString))
  }

  public func unregisterPush() async throws {
    _ = try await mutation(
      "push.unregister", PushUnregisterParams(idempotencyKey: UUID().uuidString))
  }

  /// The one answer path for the pane, notification actions and the Mac alike.
  public func answerPermission(paneId: String, requestId: String, optionId: String) async throws {
    try await waitUntilConnected()
    _ = try await mutation(
      "pane.permission.answer",
      PanePermissionAnswerParams(
        paneId: paneId, requestId: requestId, optionId: optionId,
        idempotencyKey: UUID().uuidString))
  }
}
