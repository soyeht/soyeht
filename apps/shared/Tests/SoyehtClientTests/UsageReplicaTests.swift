import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func reading(
  _ provider: UsageProvider = .claude, state: UsageState = .ok, used: Double? = 0.4,
  secondaryUsed: Double? = nil, fetchedAt: String = "2026-09-12T12:00:00Z"
) -> UsageReading {
  UsageReading(
    provider: provider, state: state,
    primary: used.map { UsageWindow(used: $0, resetsAt: "later", label: "Current") },
    secondary: secondaryUsed.map {
      UsageWindow(used: $0, resetsAt: "next week", label: "Weekly")
    },
    source: .claude_cli_usage, fetchedAt: fetchedAt)
}

@Test func usagePresentationClosesThresholdsAndStaleDisplay() {
  #expect(UsagePresentation(reading(used: 0.49)).band == .success)
  #expect(UsagePresentation(reading(used: 0.50)).band == .warning)
  #expect(UsagePresentation(reading(used: 0.80)).band == .warning)
  #expect(UsagePresentation(reading(used: 0.81)).band == .danger)
  #expect(UsagePresentation(reading(state: .stale)).percentage == "—")
  #expect(UsagePresentation(reading(state: .unavailable, used: nil)).fraction == nil)
}

@Test func usageRingsPreferWeeklyForClaudeAndCodex() {
  #expect(UsagePresentation(reading(.claude, used: 0.1, secondaryUsed: 0.6)).fraction == 0.6)
  #expect(UsagePresentation(reading(.codex, used: 0.2, secondaryUsed: 0.7)).fraction == 0.7)
  #expect(UsagePresentation(reading(.cursor, used: 0.3, secondaryUsed: 0.8)).fraction == 0.3)
  #expect(UsagePresentation(reading(.claude, used: 0.4)).fraction == 0.4)
}

@Test func hoverRefreshUsesTheContractedSixtySecondAge() throws {
  let now = try #require(ISO8601DateFormatter().date(from: "2026-09-12T12:01:00Z"))
  #expect(!UsagePresentation(reading()).needsHoverRefresh(now: now))
  #expect(
    UsagePresentation(reading(fetchedAt: "2026-09-12T11:58:59Z"))
      .needsHoverRefresh(now: now))
  #expect(UsagePresentation(reading(fetchedAt: "invalid")).needsHoverRefresh(now: now))
}

@Test func usageTimestampsAcceptProviderFractionalSeconds() {
  #expect(UsageTimestamp.date(from: "2026-09-15T12:10:15.045Z") != nil)
  #expect(UsageTimestamp.date(from: "2026-09-15T12:10:15Z") != nil)
  #expect(UsageTimestamp.date(from: "invalid") == nil)
}

@MainActor @Test func usageEventReplacesOnlyItsProviderAndKeepsCatalogueOrder() throws {
  let client = EngineClient(socket: "/tmp/sn-usage-replica/engine.sock")
  client.usageReadings = [reading(.cursor, used: 0.2), reading(.claude, used: 0.3)]
  try client.applyUsageEvent(
    .encoded(
      UsageChangedEvent(
        eventId: "1", at: "2026-09-12T12:00:01Z", provider: .codex, state: .ok,
        primary: UsageWindow(used: 0.7, resetsAt: "later", label: "Current"),
        source: .codex_app_server, fetchedAt: "2026-09-12T12:00:01Z")))

  #expect(client.usageReadings.map(\.provider) == [.claude, .codex, .cursor])
  #expect(client.usageReadings[1].primary?.used == 0.7)
}
