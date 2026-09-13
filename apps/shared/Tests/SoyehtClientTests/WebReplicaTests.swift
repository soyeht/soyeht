import SoyehtProtocol
import Testing

@testable import SoyehtClient

@MainActor @Test func webSnapshotAndEventsReflectOnlyEnginePublishedFacts() throws {
  let client = EngineClient(socket: "/tmp/web-replica.sock")
  let pane = webPane()
  client.upsertPane(pane)
  client.applyWebSnapshot(panes: [pane])
  #expect(client.webPanes.count == 1)

  try client.applyWebEvent(
    .webNavigated,
    .encoded(
      WebNavigatedEvent(
        eventId: "1", at: "2026-09-13T12:00:00Z", paneId: pane.paneId,
        navigationGeneration: "2", url: "https://example.test/next", title: "Next")))
  #expect(client.panes.first?.url == "https://example.test/next")
  #expect(client.panes.first?.title == "Next")
  #expect(client.webPanes[pane.paneId]?.navigationGeneration == "2")

  try client.applyWebEvent(
    .webConsoleCounts,
    .encoded(WebConsoleCountsEvent(paneId: pane.paneId, warnings: 3, errors: 1)))
  #expect(client.webPanes[pane.paneId]?.warnings == 3)
  #expect(client.webPanes[pane.paneId]?.errors == 1)

  try client.applyWebEvent(
    .webAgentDriving,
    .encoded(WebAgentDrivingEvent(paneId: pane.paneId, on: true, domain: "example.test")))
  #expect(client.webPanes[pane.paneId]?.agentDriving == true)
  #expect(client.webPanes[pane.paneId]?.drivingDomain == "example.test")
  try client.applyWebEvent(
    .webAgentDriving,
    .encoded(WebAgentDrivingEvent(paneId: pane.paneId, on: false)))
  #expect(client.webPanes[pane.paneId]?.agentDriving == false)
  #expect(client.webPanes[pane.paneId]?.drivingDomain == nil)

  client.applyWebSnapshot(panes: [pane])
  #expect(client.webPanes[pane.paneId]?.warnings == 0)
  #expect(client.webPanes[pane.paneId]?.agentDriving == false)
  client.applyWebSnapshot(panes: [])
  #expect(client.webPanes.isEmpty)
}

@MainActor @Test func localServerEventReplacesTheBoundedEngineList() throws {
  let client = EngineClient(socket: "/tmp/web-servers.sock")
  let server = LocalServer(
    port: 4173, pid: 42, command: "dev-server", cwd: "/workspace", title: "Preview")
  try client.applyWebEvent(
    .serversLocalChanged, .encoded(ServersLocalChangedEvent(servers: [server])))
  #expect(client.localServers.count == 1)
  #expect(client.localServers.first?.port == 4173)
  #expect(client.localServers.first?.title == "Preview")
}

@MainActor @Test func webHostResponsePreservesIdentityAndClosesItsErrorKinds() async throws {
  let command = WebCommandParams(
    paneId: "web-1", command: .snapshot, args: .object([:]), timeoutMs: 1_000,
    operationKey: "operation-1")
  let success = await EngineClient.webHostResponse(
    id: "request-1", command: command,
    host: FakeWebHost(result: WebCommandResult(paneId: "web-1", cookiesPresent: false)))
  #expect(success["id"].string == "request-1")
  #expect(try success["result"].decoded(WebCommandResult.self).paneId == "web-1")

  let stale = await EngineClient.webHostResponse(
    id: "request-2", command: command,
    host: FakeWebHost(error: .webHost(.stale_reference, "Take a new snapshot.")))
  let staleError = try stale["error"].decoded(RpcError.self)
  #expect(staleError.code == -32025)
  #expect(staleError.data.kind == .stale_reference)

  let changed = await EngineClient.webHostResponse(
    id: "request-3", command: command,
    host: FakeWebHost(result: WebCommandResult(paneId: "another-pane")))
  let changedError = try changed["error"].decoded(RpcError.self)
  #expect(changedError.code == -32018)
  #expect(changedError.data.kind == .internal_error)
}

@MainActor
private final class FakeWebHost: WebHostPort {
  private let result: WebCommandResult?
  private let error: ClientFailure?

  init(result: WebCommandResult) {
    self.result = result
    error = nil
  }

  init(error: ClientFailure) {
    result = nil
    self.error = error
  }

  func execute(_ command: WebCommandParams) async throws -> WebCommandResult {
    if let error { throw error }
    guard let result else { throw ClientFailure.invalidFrame }
    return result
  }
}

private func webPane() -> Pane {
  Pane(
    paneId: "web-1", workspaceId: "workspace-1", name: "Example", cwd: "/workspace",
    mode: .web, state: .idle, source: .protocol, confidence: 1, phrase: "viewing example.test",
    generation: 1, cols: 80, rows: 24, record: false, url: "https://example.test/",
    title: "Example",
    viewport: WebViewport(width: 1280, height: 720, preset: .responsive, zoomPercent: 100),
    incognito: false, webSession: .anonymous)
}
