/// A domain's state kept apart from `EngineClient` (ADR 0146). The client lists its replicas once
/// (`EngineClient.replicas`) and a disconnect walks the list, so a domain cannot be left out of
/// the cleanup.
@MainActor protocol ClientReplica: AnyObject {
  /// Cancels the work in flight; every disconnect does this.
  func cancelInFlight()
  /// Forgets what the engine said; a paired device does this on a disconnect.
  func reset()
}

extension EngineClient {
  /// Every replica the client owns; a disconnect cancels and clears each of them. A new domain's
  /// replica is added here and nowhere else.
  var replicas: [any ClientReplica] { [usage] }
}
