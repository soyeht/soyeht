import Foundation
import SoyehtProtocol
import SwiftUI

public enum UsageBand: Sendable {
  case success
  case warning
  case danger
}

public enum UsageTimestamp {
  public static func date(from value: String) -> Date? {
    let fractional = ISO8601DateFormatter()
    fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value)
  }
}

public struct UsagePresentation: Sendable {
  public let reading: UsageReading

  public init(_ reading: UsageReading) {
    self.reading = reading
  }

  public var displayedWindow: UsageWindow? {
    switch reading.provider {
    case .claude, .codex:
      weeklyWindow ?? reading.secondary ?? reading.primary
    case .cursor:
      reading.primary
    }
  }

  private var weeklyWindow: UsageWindow? {
    [reading.primary, reading.secondary]
      .compactMap { $0 }
      .first { $0.label.lowercased().contains("week") }
  }

  public var fraction: Double? {
    guard reading.state == .ok || reading.state == .stale,
      let used = displayedWindow?.used
    else { return nil }
    return min(max(used, 0), 1)
  }

  public var isStale: Bool { reading.state == .stale }

  public var percentage: String {
    guard let fraction else { return "—" }
    return "\(Int((fraction * 100).rounded()))%"
  }

  public var band: UsageBand? {
    guard reading.state == .ok, let fraction else { return nil }
    if fraction < 0.5 { return .success }
    if fraction <= 0.8 { return .warning }
    return .danger
  }

}

public struct ProviderGlyph: View {
  private let provider: UsageProvider

  public init(_ provider: UsageProvider) {
    self.provider = provider
  }

  public var body: some View {
    Image(provider.resourceName, bundle: .module)
      .resizable()
      .renderingMode(.template)
      .aspectRatio(contentMode: .fit)
  }
}

extension UsageProvider {
  fileprivate var resourceName: String {
    switch self {
    case .claude: "claude"
    case .codex: "codex"
    case .cursor: "cursor"
    }
  }
}

extension EngineClient {
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
        provider: event.provider, state: event.state, primary: event.primary,
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
