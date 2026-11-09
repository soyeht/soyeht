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

  /// What the engine's `welcome` says about this connection: who this client is, and what the
  /// engine offers.
  func recordWelcome(_ welcome: Welcome, client: String, deviceId: String?) {
    connectionActor = Actor(role: welcome.role, client: client, deviceId: deviceId)
    engineCapabilities = Set(welcome.capabilities)
  }

  /// Whether the engine of the last `welcome` offers `capability`. A phone asks for notification permission
  /// only when the engine offers `push`: without it no alert can reach the phone.
  public func engineOffers(_ capability: EngineCapability) -> Bool {
    engineCapabilities.contains(capability.rawValue)
  }

  /// Registers this device's APNs token; registering again replaces it. An engine that does not
  /// offer `push` is not asked: it would refuse (`push_unavailable`) and keep nothing.
  public func registerPush(token: String, environment: PushEnvironment) async throws {
    guard engineOffers(.push) else {
      throw ClientFailure.remote(
        ErrorKind.push_unavailable.rawValue, "This engine does not send push notifications")
    }
    _ = try await mutation(
      .pushRegister,
      PushRegisterParams(
        platform: .apns, token: token, environment: environment,
        idempotencyKey: UUID().uuidString))
  }

  public func unregisterPush() async throws {
    _ = try await mutation(
      .pushUnregister, PushUnregisterParams(idempotencyKey: UUID().uuidString))
  }

  /// The one answer path for the pane, notification actions and the Mac alike.
  public func answerPermission(paneId: String, requestId: String, optionId: String) async throws {
    try await waitUntilConnected()
    _ = try await mutation(
      .panePermissionAnswer,
      PanePermissionAnswerParams(
        paneId: paneId, requestId: requestId, optionId: optionId,
        idempotencyKey: UUID().uuidString))
  }
}
