import Foundation
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

@Test func humanInputBlocksAgentCommandsUntilAnExplicitHandBack() {
  var handoff = WebControlHandoff()
  let first = handoff.beginAgentCommand()
  #expect(first)
  #expect(handoff.agentCommandRunning)
  handoff.finishAgentCommand()
  #expect(!handoff.agentCommandRunning)

  handoff.noteHumanInput()
  #expect(handoff.humanHasControl)
  let blocked = handoff.beginAgentCommand()
  #expect(!blocked)
  handoff.handBack()
  #expect(!handoff.humanHasControl)
  let resumed = handoff.beginAgentCommand()
  #expect(resumed)
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
  #expect(success.message["id"].string == "request-1")
  #expect(try success.message["result"].decoded(WebCommandResult.self).paneId == "web-1")

  let stale = await EngineClient.webHostResponse(
    id: "request-2", command: command,
    host: FakeWebHost(error: .webHost(.stale_reference, "Take a new snapshot.")))
  let staleError = try stale.message["error"].decoded(RpcError.self)
  #expect(staleError.code == -32025)
  #expect(staleError.data.kind == .stale_reference)

  let changed = await EngineClient.webHostResponse(
    id: "request-3", command: command,
    host: FakeWebHost(result: WebCommandResult(paneId: "another-pane")))
  let changedError = try changed.message["error"].decoded(RpcError.self)
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

  func execute(_ command: WebCommandParams) async throws -> WebHostOutput {
    if let error { throw error }
    guard let result else { throw ClientFailure.invalidFrame }
    return WebHostOutput(result: result)
  }
}

@Test func webHostAttachmentsAreChunkedBeforeTheJsonResponse() throws {
  let id = "01234567-89ab-cdef-0123-456789abcdef"
  let bytes = Data(repeating: 0x5a, count: ProtocolLimits.attachmentPayloadBytes + 3)
  let reference = AttachmentRef(
    attachmentId: id, mimeType: "image/png", name: "screenshot.png",
    bytes: String(bytes.count))
  let response = WebHostResponse(
    message: .object(["jsonrpc": .string("2.0"), "id": .string("request-1")]),
    attachments: [WebHostAttachment(reference: reference, bytes: bytes)])
  let frames = try response.frames()
  #expect(frames.map(\.kind) == [3, 3, 1])
  #expect(frames[0].payload.count == ProtocolLimits.attachmentPayloadBytes)
  #expect(frames[1].payload == Data(repeating: 0x5a, count: 3))
  #expect(frames[0].header == frames[1].header)
  #expect(
    try JSONDecoder().decode(JsonValue.self, from: frames[2].payload)["id"].string == "request-1")
}

@Test func promptPayloadCarriesTwentyOrderedImagesBeforeItsJsonRequest() throws {
  let attachments = (0..<20).map { index in
    let id = String(format: "00000000-0000-0000-0000-%012d", index)
    let bytes = Data([UInt8(index)])
    return WebHostAttachment(
      reference: AttachmentRef(
        attachmentId: id, mimeType: "image/png", name: "element-\(index).png",
        bytes: String(bytes.count)),
      bytes: bytes)
  }
  let frames = try WirePayload(
    message: .object(["method": .string("pane.prompt")]), attachments: attachments
  ).frames()
  #expect(frames.count == 21)
  #expect(frames.dropLast().allSatisfy { $0.kind == 3 })
  #expect(frames.dropLast().map { $0.payload.first } == (0..<20).map(UInt8.init))
  #expect(frames.last?.kind == 1)
}

@Test func selectedElementDraftBuildsOneMarkdownBodyAndKeepsImageOrder() {
  let images = [0, 1].map { index in
    let id = String(format: "10000000-0000-0000-0000-%012d", index)
    let bytes = Data([UInt8(index)])
    return WebHostAttachment(
      reference: AttachmentRef(
        attachmentId: id, mimeType: "image/png", name: "element-\(index).png",
        bytes: String(bytes.count)), bytes: bytes)
  }
  let elements = images.enumerated().map { index, image in
    WebPickedElement(
      selector: "button:nth-of-type(\(index + 1))", path: ["html", "body", "button"],
      outerHtml: "<button>Save</button>",
      styles: WebComputedStyles(
        boxSizing: "border-box", color: "rgb(1, 2, 3)", background: "white",
        font: "12px sans-serif", border: "none", borderRadius: "6px", display: "block",
        position: "static"),
      accessibility: WebAccessibility(role: "button", name: "Save", state: "enabled"),
      viewportRect: WebRect(x: 10, y: 20, width: 96, height: 32),
      pageRect: WebRect(x: 10, y: 120, width: 96, height: 32), image: image.reference,
      component: WebComponentSource(
        framework: "UI", component: "SaveButton", sourceFile: "src/save.tsx", line: 42))
  }
  let draft = AgentWebAttachmentDraft(
    sourcePaneId: "web-1",
    package: WebElementPackage(url: "https://example.test/", title: "Example", elements: elements),
    images: images)
  #expect(draft.markdown.contains("## Selected page elements"))
  #expect(draft.markdown.contains("untrusted context"))
  #expect(draft.markdown.utf8.count <= 32 * 1_024)
  #expect(draft.markdown.contains("src/save.tsx:42"))
  #expect(draft.markdown.contains("### Element 2"))
  #expect(draft.images.map(\.bytes) == [Data([0]), Data([1])])
}

@MainActor @Test func selectedElementsTargetTheLastFocusedAgentInTheirWorkspace() {
  let client = EngineClient(socket: "/tmp/web-pick.sock")
  let first = agentPane("agent-1", name: "First")
  let last = agentPane("agent-2", name: "Last")
  client.upsertPane(first)
  client.upsertPane(last)
  client.select(last.paneId)
  #expect(client.preferredAgentPane(in: "workspace-1")?.paneId == last.paneId)

  let bytes = Data([1, 2, 3])
  let reference = AttachmentRef(
    attachmentId: "20000000-0000-0000-0000-000000000000", mimeType: "image/png",
    name: "selected-element.png", bytes: String(bytes.count))
  let element = WebPickedElement(
    selector: "button", path: ["html", "body", "button"], outerHtml: "<button>Save</button>",
    styles: WebComputedStyles(
      boxSizing: "border-box", color: "black", background: "white", font: "12px sans-serif",
      border: "none", borderRadius: "0px", display: "block", position: "static"),
    accessibility: WebAccessibility(role: "button", name: "Save", state: "enabled"),
    viewportRect: WebRect(x: 1, y: 2, width: 3, height: 4),
    pageRect: WebRect(x: 1, y: 2, width: 3, height: 4), image: reference)
  client.attachWebElements(
    from: "web-1",
    package: WebElementPackage(
      url: "https://example.test/", title: "Example", elements: [element]),
    images: [WebHostAttachment(reference: reference, bytes: bytes)], to: last.paneId)
  #expect(client.agentWebDrafts[last.paneId]?.sourcePaneId == "web-1")
  #expect(client.selectedPaneId == last.paneId)
  client.discardWebAttachmentDraft(last.paneId)
  #expect(client.agentWebDrafts.isEmpty)
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

private func agentPane(_ paneId: String, name: String) -> Pane {
  Pane(
    paneId: paneId, workspaceId: "workspace-1", name: name, cwd: "/workspace",
    mode: .agent, state: .idle, source: .protocol, confidence: 1, phrase: "Ready",
    generation: 1, cols: 80, rows: 24, record: true,
    agent: AgentSession(
      harness: "agent", version: "1", fingerprint: "fingerprint", options: [], authMethods: [],
      permission: .ask, yolo: false, queue: [], draft: false, role: .worker))
}
