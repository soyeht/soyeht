import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

#if os(macOS)
  import Darwin
#endif

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

@MainActor
private final class UsageRefreshProbe {
  private(set) var calls = 0
  let result: UsageRefreshResult

  init(result: UsageRefreshResult) {
    self.result = result
  }

  func run(_ provider: UsageProvider?) async throws -> UsageRefreshResult {
    calls += 1
    try await Task.sleep(for: .milliseconds(25))
    return result
  }
}

@Test func usagePresentationClosesThresholdsAndStaleDisplay() {
  #expect(UsagePresentation(reading(used: 0.49)).band == .success)
  #expect(UsagePresentation(reading(used: 0.50)).band == .warning)
  #expect(UsagePresentation(reading(used: 0.80)).band == .warning)
  #expect(UsagePresentation(reading(used: 0.81)).band == .danger)
  #expect(UsagePresentation(reading(state: .stale)).percentage == "40%")
  #expect(UsagePresentation(reading(state: .stale)).band == nil)
  #expect(UsagePresentation(reading(state: .stale)).isStale)
  #expect(UsagePresentation(reading(state: .unavailable, used: nil)).fraction == nil)
}

@Test func usageRingsPreferWeeklyForClaudeAndCodex() {
  #expect(UsagePresentation(reading(.claude, used: 0.1, secondaryUsed: 0.6)).fraction == 0.6)
  #expect(UsagePresentation(reading(.codex, used: 0.2, secondaryUsed: 0.7)).fraction == 0.7)
  #expect(UsagePresentation(reading(.cursor, used: 0.3, secondaryUsed: 0.8)).fraction == 0.3)
  #expect(UsagePresentation(reading(.claude, used: 0.4)).fraction == 0.4)
}

@Test func codexRingFindsWeeklyInEitherWindowPosition() {
  let reading = UsageReading(
    provider: .codex, state: .ok,
    primary: UsageWindow(used: 0.8, resetsAt: "next week", label: "Weekly"),
    secondary: UsageWindow(used: 0.2, resetsAt: "soon", label: "Current window"),
    source: .codex_app_server, fetchedAt: "2026-09-15T12:00:00Z")
  #expect(UsagePresentation(reading).fraction == 0.8)
  #expect(UsagePresentation(reading).displayedWindow?.label == "Weekly")
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

@MainActor @Test func usageRefreshCoalescesConcurrentCallsAndHonorsTheWireInterval() async throws {
  let fetchedAt = "2026-09-15T12:00:00Z"
  let refreshed = reading(.claude, used: 0.6, fetchedAt: fetchedAt)
  let probe = UsageRefreshProbe(result: UsageRefreshResult(providers: [refreshed]))
  let client = EngineClient(socket: "/tmp/sn-usage-refresh/engine.sock")
  let now = try #require(UsageTimestamp.date(from: fetchedAt))

  async let first: Void = client.refreshUsage(.claude, now: now) { provider in
    try await probe.run(provider)
  }
  async let second: Void = client.refreshUsage(.claude, now: now) { provider in
    try await probe.run(provider)
  }
  _ = try await (first, second)

  #expect(probe.calls == 1)
  #expect(client.usageReadings.first?.primary?.used == 0.6)

  try await client.refreshUsage(.claude, now: now.addingTimeInterval(29)) { provider in
    try await probe.run(provider)
  }
  #expect(probe.calls == 1)

  try await client.refreshUsage(.claude, now: now.addingTimeInterval(30)) { provider in
    try await probe.run(provider)
  }
  #expect(probe.calls == 2)
}

@MainActor @Test func usageRefreshWaitsForAnAuthorizedConnection() async {
  let client = EngineClient(socket: "/tmp/sn-usage-before-hello/engine.sock")
  do {
    try await client.refreshUsage(.claude)
    Issue.record("Usage refresh reached the wire before hello completed")
  } catch is ClientFailure {
    #expect(!client.connected)
  } catch {
    Issue.record("Unexpected error: \(error)")
  }
}

#if os(macOS)
  @MainActor @Test func claudeCacheObserverHandlesAtomicRenameInARealDirectory() async throws {
    let directory = FileManager.default.temporaryDirectory
      .appendingPathComponent("soyeht-usage-observer-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }

    let cache = directory.appendingPathComponent(".claude.json")
    let replacement = directory.appendingPathComponent("replacement.json")
    try Data("{}".utf8).write(to: cache)
    let observer = ClaudeUsageCacheObserver(cacheURL: cache)
    var changes = 0
    observer.start { changes += 1 }

    try Data("{\"cachedUsageUtilization\":{}}".utf8).write(to: replacement)
    let renamed = replacement.path.withCString { source in
      cache.path.withCString { destination in Darwin.rename(source, destination) }
    }
    #expect(renamed == 0)
    for _ in 0..<100 where changes == 0 {
      try await Task.sleep(for: .milliseconds(20))
    }
    #expect(changes == 1)
    observer.stop()
  }
#endif
