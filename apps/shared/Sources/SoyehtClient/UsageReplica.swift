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
    let result = try await request("usage.list", .encoded(UsageListParams()))
      .decoded(UsageListResult.self)
    usageReadings = result.providers
  }

  public func refreshUsage(_ provider: UsageProvider? = nil) async throws {
    guard connected else { throw ClientFailure.disconnected }
    try await refreshUsage(provider, now: Date()) { [self] provider in
      try await requestUsageRefresh(provider)
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
    let timeout = provider == nil ? 35.0 : 12.0
    return try await mutation(
      "usage.refresh",
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
        secondary: event.secondary, source: event.source, fetchedAt: event.fetchedAt))
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
    }
  }
}

extension UsageProvider {
  public var displayName: String {
    switch self {
    case .claude: "Claude"
    case .codex: "Codex"
    case .cursor: "Cursor"
    }
  }
}

extension UsageSource {
  public var displayName: String {
    switch self {
    case .claude_cli_usage: "Claude Code CLI"
    case .claude_oauth_endpoint: "Claude account endpoint"
    case .claude_desktop_cache: "Claude Code cache"
    case .codex_app_server: "Codex app server"
    case .codex_account_endpoint: "Codex account endpoint"
    case .cursor_current_period: "Cursor current period"
    }
  }
}
