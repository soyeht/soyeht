import Foundation
import SoyehtProtocol

enum UsageDetailsRefreshPolicy {
  static func shouldRefresh(_ reading: UsageReading) -> Bool {
    reading.state == .stale
      || (reading.state == .unavailable && reading.unavailableReason == .not_checked)
  }
}

extension EngineClient {
  public func refreshUsageOnDetailsClick(_ provider: UsageProvider) {
    guard let reading = usageReadings.first(where: { $0.provider == provider }),
      UsageDetailsRefreshPolicy.shouldRefresh(reading)
    else { return }
    perform { try await self.refreshUsageAfterRateLimitOnce(provider) }
  }

  func loadUsage() async throws {
    let result = try await request(.usageList, .encoded(UsageListParams()))
      .decoded(UsageListResult.self)
    usageReadings = result.providers
  }

  public func refreshUsage(_ provider: UsageProvider? = nil) async throws {
    guard connected else { throw ClientFailure.disconnected }
    try await refreshUsage(provider, now: Date()) { [self] provider in
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
    try await refreshUsageAfterRateLimitOnce(
      wait: { seconds in try await Task.sleep(for: .seconds(seconds)) },
      operation: { [self] in try await refreshUsage(provider) })
  }

  func refreshUsageAfterRateLimitOnce(
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

  func refreshUsage(
    _ provider: UsageProvider?, now: Date,
    operation: @escaping @MainActor @Sendable (UsageProvider?) async throws -> UsageRefreshResult
  ) async throws {
    if let task = activeUsageRefreshTask(for: provider) {
      applyUsageRefresh(try await task.value)
      return
    }
    guard usageRefreshIsDue(provider, now: now) else { return }

    let key = provider?.rawValue ?? "all"
    let task = Task<UsageRefreshResult, Error> { @MainActor in
      try await operation(provider)
    }
    usageRefreshTasks[key] = task
    do {
      let result = try await task.value
      usageRefreshTasks.removeValue(forKey: key)
      applyUsageRefresh(result)
    } catch {
      usageRefreshTasks.removeValue(forKey: key)
      throw error
    }
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

  private func activeUsageRefreshTask(
    for provider: UsageProvider?
  ) -> Task<UsageRefreshResult, Error>? {
    if let all = usageRefreshTasks["all"] { return all }
    if let provider { return usageRefreshTasks[provider.rawValue] }
    return usageRefreshTasks.values.first
  }

  private func usageRefreshIsDue(_ provider: UsageProvider?, now: Date) -> Bool {
    let candidates =
      provider.map { wanted in
        usageReadings.filter { $0.provider == wanted }
      } ?? usageReadings
    return !candidates.contains { reading in
      guard let fetched = UsageTimestamp.date(from: reading.fetchedAt) else { return false }
      return now.timeIntervalSince(fetched) < Double(ProtocolLimits.usageRefreshMinIntervalSec)
    }
  }

  private func applyUsageRefresh(_ result: UsageRefreshResult) {
    for reading in result.providers { replaceUsage(reading) }
  }

  func applyUsageEvent(_ params: JsonValue) throws {
    let event = try params.decoded(UsageChangedEvent.self)
    replaceUsage(
      UsageReading(
        provider: event.provider, state: event.state,
        unavailableReason: event.unavailableReason, primary: event.primary,
        secondary: event.secondary, activity: event.activity, source: event.source,
        fetchedAt: event.fetchedAt))
  }

  private func replaceUsage(_ reading: UsageReading) {
    usageReadings.removeAll { $0.provider == reading.provider }
    usageReadings.append(reading)
    usageReadings.sort { $0.provider.order < $1.provider.order }
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
