import Foundation
import Observation
import SoyehtProtocol

enum UsageDetailsRefreshPolicy {
  static func shouldRefresh(_ reading: UsageReading) -> Bool {
    reading.state == .stale
      || (reading.state == .unavailable && reading.unavailableReason == .not_checked)
  }
}

/// What the engine last said about each provider's usage, and the rules that keep the readings
/// fresh: one reading per provider in catalogue order, one refresh in flight per request, and no
/// refresh inside the wire's minimum interval. It owns no connection: `EngineClient` hands it the
/// engine's answers and the request to refresh with.
@MainActor @Observable
public final class UsageReplica {
  public internal(set) var readings: [UsageReading] = []
  @ObservationIgnored private var refreshes: [String: Task<UsageRefreshResult, Error>] = [:]

  /// Replaces every reading with the engine's list.
  func load(_ providers: [UsageReading]) {
    readings = providers
  }

  func apply(_ event: UsageChangedEvent) {
    replace(
      UsageReading(
        provider: event.provider, state: event.state,
        unavailableReason: event.unavailableReason, primary: event.primary,
        secondary: event.secondary, activity: event.activity, source: event.source,
        fetchedAt: event.fetchedAt))
  }

  private func apply(_ result: UsageRefreshResult) {
    for reading in result.providers { replace(reading) }
  }

  private func replace(_ reading: UsageReading) {
    readings.removeAll { $0.provider == reading.provider }
    readings.append(reading)
    readings.sort { $0.provider.order < $1.provider.order }
  }

  /// A paired device forgets the readings when the connection goes.
  func reset() {
    readings = []
  }

  func cancelRefreshes() {
    for task in refreshes.values { task.cancel() }
    refreshes.removeAll()
  }

  /// Whether opening the details of `provider` should refresh its reading.
  func refreshesOnDetailsClick(_ provider: UsageProvider) -> Bool {
    guard let reading = readings.first(where: { $0.provider == provider }) else { return false }
    return UsageDetailsRefreshPolicy.shouldRefresh(reading)
  }

  /// Refreshes `provider` (every provider for `nil`) with `operation`, joining a refresh already
  /// in flight and skipping one the wire's minimum interval forbids.
  func refresh(
    _ provider: UsageProvider?, now: Date,
    operation: @escaping @MainActor @Sendable (UsageProvider?) async throws -> UsageRefreshResult
  ) async throws {
    if let task = activeRefresh(for: provider) {
      apply(try await task.value)
      return
    }
    guard refreshIsDue(provider, now: now) else { return }

    let key = provider?.rawValue ?? "all"
    let task = Task<UsageRefreshResult, Error> { @MainActor in
      try await operation(provider)
    }
    refreshes[key] = task
    do {
      let result = try await task.value
      refreshes.removeValue(forKey: key)
      apply(result)
    } catch {
      refreshes.removeValue(forKey: key)
      throw error
    }
  }

  /// Runs `operation`; when the engine rate-limits it with a delay, waits that long and runs it
  /// once more.
  func retryingRateLimitOnce(
    wait: @escaping @MainActor @Sendable (UInt32) async throws -> Void,
    operation: @escaping @MainActor @Sendable () async throws -> Void
  ) async throws {
    do {
      try await operation()
    } catch ClientFailure.remote(let kind, _, let retryAfterSec)
      where kind == ErrorKind.rate_limited.rawValue && retryAfterSec != nil
    {
      try await wait(retryAfterSec!)
      try await operation()
    }
  }

  private func activeRefresh(for provider: UsageProvider?) -> Task<UsageRefreshResult, Error>? {
    if let all = refreshes["all"] { return all }
    if let provider { return refreshes[provider.rawValue] }
    return refreshes.values.first
  }

  private func refreshIsDue(_ provider: UsageProvider?, now: Date) -> Bool {
    let candidates =
      provider.map { wanted in
        readings.filter { $0.provider == wanted }
      } ?? readings
    return !candidates.contains { reading in
      guard let fetched = UsageTimestamp.date(from: reading.fetchedAt) else { return false }
      return now.timeIntervalSince(fetched) < Double(ProtocolLimits.usageRefreshMinIntervalSec)
    }
  }
}

extension UsageProvider {
  fileprivate var order: Int {
    switch self {
    case .claude: 0
    case .codex: 1
    case .cursor: 2
    case .opencode: 3
    }
  }
}

extension UsageSource {
  public var displayName: String {
    switch self {
    case .claude_oauth_endpoint: "Claude Code login (OAuth)"
    case .claude_cli_usage: "Claude Code /usage"
    case .claude_web: "claude.ai browser session"
    case .claude_desktop_cache: "Claude Code cache"
    case .codex_access_token: "Codex access token"
    case .codex_account_endpoint: "Codex login (ChatGPT)"
    case .codex_app_server: "Codex app server"
    case .cursor_app_login: "Cursor app login"
    case .cursor_browser_cookies: "cursor.com browser session"
    case .cursor_current_period: "Cursor current period"
    case .opencode_go_allowance: "OpenCode Go allowance"
    case .opencode_zai_plan: "GLM Coding Plan quota"
    case .opencode_local_history: "OpenCode history on this device"
    }
  }
}
