import Foundation
import SoyehtProtocol
import SwiftUI

public enum UsageBand: Sendable {
  case success
  case warning
  case danger
}

public struct UsagePresentation: Sendable {
  public let reading: UsageReading

  public init(_ reading: UsageReading) {
    self.reading = reading
  }

  public var fraction: Double? {
    guard reading.state == .ok, let used = reading.primary?.used else { return nil }
    return min(max(used, 0), 1)
  }

  public var percentage: String {
    guard let fraction else { return "—" }
    return "\(Int((fraction * 100).rounded()))%"
  }

  public var band: UsageBand? {
    guard let fraction else { return nil }
    if fraction < 0.5 { return .success }
    if fraction <= 0.8 { return .warning }
    return .danger
  }

  public func needsHoverRefresh(now: Date = Date()) -> Bool {
    let timestamp = ISO8601DateFormatter()
    guard reading.state == .ok,
      let fetched = timestamp.date(from: reading.fetchedAt)
    else { return true }
    return now.timeIntervalSince(fetched) > 60
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
    let timeout = provider == nil ? 35.0 : 12.0
    let result = try await mutation(
      "usage.refresh",
      UsageRefreshParams(provider: provider, idempotencyKey: UUID().uuidString),
      timeoutSeconds: timeout
    ).decoded(UsageRefreshResult.self)
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
    case .claude_desktop_cache: "Claude desktop cache"
    case .codex_app_server: "Codex app server"
    case .codex_account_endpoint: "Codex account endpoint"
    case .cursor_current_period: "Cursor current period"
    }
  }
}
