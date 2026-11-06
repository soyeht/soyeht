import Foundation
import SoyehtProtocol

/// The usage domain on the wire: `EngineClient` asks, `UsageReplica` keeps the answers.
extension EngineClient {
  public func refreshUsageOnDetailsClick(_ provider: UsageProvider) {
    guard usage.refreshesOnDetailsClick(provider) else { return }
    perform { try await self.refreshUsageAfterRateLimitOnce(provider) }
  }

  func loadUsage() async throws {
    let result = try await request(.usageList, .encoded(UsageListParams()))
      .decoded(UsageListResult.self)
    usage.load(result.providers)
  }

  public func refreshUsage(_ provider: UsageProvider? = nil) async throws {
    guard connected else { throw ClientFailure.disconnected }
    try await usage.refresh(provider, now: Date()) { [self] provider in
      try await requestUsageRefresh(provider)
    }
  }

  /// Refreshes usage in the background: the rings keep their last reading, and a failed
  /// refresh goes to the log instead of being dropped. Cancellation stays silent.
  public func refreshUsageInBackground(
    _ provider: UsageProvider? = nil, retryingRateLimit: Bool = false
  ) async {
    do {
      if retryingRateLimit {
        try await refreshUsageAfterRateLimitOnce(provider)
      } else {
        try await refreshUsage(provider)
      }
    } catch is CancellationError {
    } catch {
      clientLogger.notice(
        "usage_refresh_failed provider=\(provider?.rawValue ?? "all", privacy: .public) error=\(String(describing: error), privacy: .public)"
      )
    }
  }

  public func refreshUsageAfterRateLimitOnce(_ provider: UsageProvider? = nil) async throws {
    try await usage.retryingRateLimitOnce(
      wait: { seconds in try await Task.sleep(for: .seconds(seconds)) },
      operation: { [self] in try await refreshUsage(provider) })
  }

  func applyUsageEvent(_ params: JsonValue) throws {
    usage.apply(try params.decoded(UsageChangedEvent.self))
  }

  private func requestUsageRefresh(_ provider: UsageProvider?) async throws -> UsageRefreshResult {
    // The engine walks each provider's fallback chain (network, keychain prompt, hidden CLI
    // session) in parallel; a chain bounds its own waits well inside this.
    let timeout = 150.0
    return try await mutation(
      .usageRefresh,
      UsageRefreshParams(provider: provider, idempotencyKey: UUID().uuidString),
      timeoutSeconds: timeout
    ).decoded(UsageRefreshResult.self)
  }
}
