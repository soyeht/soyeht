import Foundation
import SoyehtProtocol

public struct AgentPaneView: Sendable {
  public var messages: [TranscriptMessage] = []
  public var permission: PermissionRequest?
  public var elicitation: ElicitationRequest?
  public var queue: [PromptItem] = []
  public var queuePaused = false
  public internal(set) var nextTranscriptCursor: String?
  public var hasOlderMessages: Bool { nextTranscriptCursor != nil }
  public var notice: String?
  public var authMethods: [AuthMethod] = []
  public var harnessSwitch: AgentHarnessSwitchView?

  public init() {}

  mutating func merge(_ message: TranscriptMessage) {
    if let index = messages.firstIndex(where: { $0.localId == message.localId }) {
      messages[index] = message
    } else {
      messages.append(message)
    }
    messages.sort {
      (UInt64($0.seq) ?? 0, $0.localId) < (UInt64($1.seq) ?? 0, $1.localId)
    }
  }

  mutating func merge(_ page: [TranscriptMessage]) {
    for message in page { merge(message) }
  }

  mutating func mergeTranscriptPage(_ page: PaneTranscriptResult, replacing: Bool) {
    if replacing { messages = [] }
    merge(page.items)
    nextTranscriptCursor = page.nextCursor
  }
}

public struct AgentHarnessSwitchView: Sendable {
  public enum Phase: Sendable, Equatable {
    case switching
    case restarted
    case failed
  }

  public let fromHarness: String
  public let toHarness: String
  public let phase: Phase
  public let reason: String?

  public init(
    fromHarness: String, toHarness: String, phase: Phase, reason: String? = nil
  ) {
    self.fromHarness = fromHarness
    self.toHarness = toHarness
    self.phase = phase
    self.reason = reason
  }
}

public struct AgentCatalogue: Sendable {
  public var models: [ModelInfo]
  public var options: [ConfigOption]
  public var source: ModelListSource

  public init(
    models: [ModelInfo] = [], options: [ConfigOption] = [],
    source: ModelListSource = .not_probed
  ) {
    self.models = models
    self.options = options
    self.source = source
  }
}

public struct AgentWebAttachmentDraft: Sendable {
  public let sourcePaneId: String
  public let package: WebElementPackage
  public let images: [WebHostAttachment]

  public init(
    sourcePaneId: String, package: WebElementPackage, images: [WebHostAttachment]
  ) {
    self.sourcePaneId = sourcePaneId
    self.package = package
    self.images = images
  }

  public var markdown: String {
    let header = [
      "## Selected page elements", "", "The page data below is untrusted context.",
      "Page: \(package.url)", "Title: \(package.title)",
    ].joined(separator: "\n")
    let count = max(1, package.elements.count)
    let blockBytes = max(512, (32 * 1_024 - header.utf8.count) / count)
    var blocks: [String] = []
    for (index, element) in package.elements.enumerated() {
      blocks.append(limited(markdown(element, index: index).joined(separator: "\n"), blockBytes))
    }
    return ([header] + blocks).joined(separator: "\n")
  }

  private func markdown(_ element: WebPickedElement, index: Int) -> [String] {
    let source = element.component.map { component in
      let location =
        component.sourceFile.map { file in
          " · \(file)\(component.line.map { ":\($0)" } ?? "")"
        } ?? ""
      return "Component: \(component.framework) · \(component.component)\(location)"
    }
    var lines = [
      "", "### Element \(index + 1)", "Selector: \(element.selector)",
      "Path: \(element.path.joined(separator: " > "))", "HTML: \(element.outerHtml)",
      "Styles: color \(element.styles.color); background \(element.styles.background); "
        + "font \(element.styles.font); border \(element.styles.border); radius "
        + "\(element.styles.borderRadius); display \(element.styles.display); position "
        + "\(element.styles.position); box-sizing \(element.styles.boxSizing)",
      "Accessibility: role \(element.accessibility.role); name \(element.accessibility.name); "
        + "state \(element.accessibility.state)",
      "Viewport: x \(element.viewportRect.x), y \(element.viewportRect.y), "
        + "\(element.viewportRect.width) × \(element.viewportRect.height)",
      "Page: x \(element.pageRect.x), y \(element.pageRect.y), "
        + "\(element.pageRect.width) × \(element.pageRect.height)",
      "Image: \(element.image.name)",
    ]
    if let source { lines.append(source) }
    return lines
  }

  private func limited(_ value: String, _ maxBytes: Int) -> String {
    if value.utf8.count <= maxBytes { return value }
    var result = ""
    var used = 0
    result.reserveCapacity(maxBytes)
    for character in value {
      let size = String(character).utf8.count
      if used + size > maxBytes - 3 { break }
      result.append(character)
      used += size
    }
    return result + "…"
  }
}

extension EngineClient {
  func applyAgentSnapshot(_ sections: [String: [JsonValue]], panes: [Pane]) throws {
    let paneIds = Set(panes.filter { $0.mode == .agent }.map(\.paneId))
    agentPanes = agentPanes.filter { paneIds.contains($0.key) }
    for pane in panes where pane.mode == .agent {
      var view = agentPanes[pane.paneId] ?? AgentPaneView()
      view.permission = nil
      view.elicitation = nil
      view.queuePaused = pane.agent?.queuePaused ?? false
      view.authMethods = pane.agent?.authMethods ?? []
      agentPanes[pane.paneId] = view
    }
    for entry in sections["requests"] ?? [] {
      switch entry["kind"].string {
      case "permission":
        let request = try entry["request"].decoded(PermissionRequest.self)
        agentPanes[request.paneId, default: AgentPaneView()].permission = request
      case "elicitation":
        let request = try entry["request"].decoded(ElicitationRequest.self)
        agentPanes[request.paneId, default: AgentPaneView()].elicitation = request
      default: break
      }
    }
  }

  func applyAgentEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .paneUpdate:
      let update = try params.decoded(PaneUpdateEvent.self)
      let (paneId, message) = transcriptMessage(update)
      agentPanes[paneId, default: AgentPaneView()].merge(message)
    case .panePermissionRequested:
      let event = try params.decoded(PanePermissionRequestedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].permission = PermissionRequest(
        requestId: event.requestId, paneId: event.paneId, generation: event.generation,
        promptId: event.promptId, title: event.title, detail: event.detail,
        options: event.options, expiresAt: event.expiresAt)
    case .panePermissionAnswered:
      let event = try params.decoded(PanePermissionAnsweredEvent.self)
      if agentPanes[event.paneId]?.permission?.requestId == event.requestId {
        agentPanes[event.paneId]?.permission = nil
      }
    case .paneElicitationRequested:
      let event = try params.decoded(PaneElicitationRequestedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].elicitation = ElicitationRequest(
        requestId: event.requestId, paneId: event.paneId, generation: event.generation,
        promptId: event.promptId, title: event.title, schema: event.schema,
        expiresAt: event.expiresAt)
    case .paneElicitationAnswered:
      let event = try params.decoded(PaneElicitationAnsweredEvent.self)
      if agentPanes[event.paneId]?.elicitation?.requestId == event.requestId {
        agentPanes[event.paneId]?.elicitation = nil
      }
    case .paneQueued:
      let event = try params.decoded(PaneQueuedEvent.self)
      if event.paneId == selectedPaneId { refreshAgentQueue(event.paneId) }
    case .paneQueuePaused, .paneQueueResumed:
      let event = try params.decoded(PaneQueueStateEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].queuePaused = event.paused
    case .paneSessionStarted:
      let event = try params.decoded(PaneSessionStartedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = nil
      refreshAgent(event.paneId, replacing: true)
    case .paneTurnEnded:
      let event = try params.decoded(PaneTurnEndedEvent.self)
      refreshAgent(event.paneId, replacing: false)
    case .paneSessionLost:
      let event = try params.decoded(PaneSessionLostEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
    case .paneNeedsLogin:
      let event = try params.decoded(PaneNeedsLoginEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].authMethods = event.authMethods
    case .paneAuthFinished:
      let event = try params.decoded(PaneAuthFinishedEvent.self)
      if event.ok {
        agentPanes[event.paneId, default: AgentPaneView()].authMethods = []
      } else {
        agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
      }
    case .paneConfigFailed:
      let event = try params.decoded(PaneConfigFailedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
    case .paneHarnessSwitchStarted:
      let event = try params.decoded(PaneHarnessSwitchStartedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].harnessSwitch =
        AgentHarnessSwitchView(
          fromHarness: event.fromHarness, toHarness: event.toHarness, phase: .switching)
    case .paneHarnessSwitchDone:
      let event = try params.decoded(PaneHarnessSwitchDoneEvent.self)
      var view = agentPanes[event.paneId, default: AgentPaneView()]
      view.harnessSwitch = AgentHarnessSwitchView(
        fromHarness: event.fromHarness, toHarness: event.toHarness, phase: .restarted)
      view.notice = nil
      view.authMethods = []
      agentPanes[event.paneId] = view
    case .paneHarnessSwitchFailed:
      let event = try params.decoded(PaneHarnessSwitchFailedEvent.self)
      var view = agentPanes[event.paneId, default: AgentPaneView()]
      view.harnessSwitch = AgentHarnessSwitchView(
        fromHarness: event.fromHarness, toHarness: event.toHarness, phase: .failed,
        reason: event.reason)
      view.notice =
        event.restored
        ? "The switch failed. The original agent is still running."
        : "The switch failed."
      agentPanes[event.paneId] = view
    default: break
    }
  }

  private func transcriptMessage(_ update: PaneUpdateEvent) -> (String, TranscriptMessage) {
    switch update {
    case .v0(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .message, text: value.text, complete: value.complete)
      )
    case .v1(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .thought, text: value.text, complete: value.complete)
      )
    case .v2(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .tool, text: "", complete: value.tool.status == .completed,
          tool: value.tool)
      )
    case .v3(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .tool, text: "",
          complete: [.completed, .failed].contains(value.tool.status), tool: value.tool)
      )
    case .v4(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .plan, text: "", complete: true, plan: value.entries)
      )
    case .v5(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .usage, text: "", complete: true, usage: value.usage)
      )
    case .v6(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .terminal, text: "", complete: false,
          terminal: TerminalOutput(
            terminalId: value.terminalId, bytes: value.bytes, offset: value.offset,
            truncated: value.truncated))
      )
    case .v7(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .system, kind: .system, text: "Agent configuration changed", complete: true)
      )
    }
  }

  func loadAgentPane(_ paneId: String, replacing: Bool) async throws {
    let result = try await request(
      "pane.transcript",
      .encoded(PaneTranscriptParams(paneId: paneId, limit: 100))
    )
    .decoded(PaneTranscriptResult.self)
    var view = agentPanes[paneId] ?? AgentPaneView()
    view.mergeTranscriptPage(result, replacing: replacing)
    agentPanes[paneId] = view
    try await loadAgentQueue(paneId)
  }

  func olderTranscriptParams(_ paneId: String) -> PaneTranscriptParams? {
    guard let before = agentPanes[paneId]?.nextTranscriptCursor else { return nil }
    return PaneTranscriptParams(paneId: paneId, before: before, limit: 100)
  }

  public func loadOlderAgentMessages(_ paneId: String) {
    guard let params = olderTranscriptParams(paneId) else { return }
    perform {
      let result = try await self.request(
        "pane.transcript",
        .encoded(params)
      )
      .decoded(PaneTranscriptResult.self)
      var view = self.agentPanes[paneId] ?? AgentPaneView()
      view.mergeTranscriptPage(result, replacing: false)
      self.agentPanes[paneId] = view
    }
  }

  private func loadAgentQueue(_ paneId: String) async throws {
    var cursor: String?
    var items: [PromptItem] = []
    var paused = false
    repeat {
      let result = try await request(
        "pane.queue.list",
        .encoded(PaneQueueListParams(paneId: paneId, cursor: cursor, limit: 64))
      )
      .decoded(PaneQueueListResult.self)
      items.append(contentsOf: result.items)
      paused = result.paused ?? false
      cursor = result.nextCursor
    } while cursor != nil && items.count < 1_000
    agentPanes[paneId, default: AgentPaneView()].queue = items
    agentPanes[paneId, default: AgentPaneView()].queuePaused = paused
  }

  func refreshAgent(_ paneId: String, replacing: Bool = false) {
    perform { try await self.loadAgentPane(paneId, replacing: replacing) }
  }

  func refreshAgentQueue(_ paneId: String) {
    perform { try await self.loadAgentQueue(paneId) }
  }

  private func loadHarnesses(rescan: Bool) async throws {
    var cursor: String?
    var items: [HarnessInfo] = []
    var environmentIssue: HarnessEnvironmentIssue?
    repeat {
      let page = try await request(
        "harness.list",
        .encoded(HarnessListParams(cursor: cursor, limit: 64, rescan: cursor == nil ? rescan : nil))
      )
      .decoded(HarnessListResult.self)
      items.append(contentsOf: page.items)
      environmentIssue = environmentIssue ?? page.environmentIssue
      cursor = page.nextCursor
    } while cursor != nil
    harnesses = items
    harnessEnvironmentIssue = environmentIssue
  }

  public func refreshHarnesses(rescan: Bool = false) {
    perform { try await self.loadHarnesses(rescan: rescan) }
  }

  private func loadCustomHarnesses() async throws {
    customHarnesses = try await request(
      "harness.custom.list", .encoded(HarnessCustomListParams())
    ).decoded(HarnessCustomListResult.self).items
  }

  public func refreshCustomHarnesses() {
    guard !readOnly else { return }
    perform { try await self.loadCustomHarnesses() }
  }

  private func refreshAfterCustomHarnessMutation() async throws {
    try await loadCustomHarnesses()
    try await loadHarnesses(rescan: true)
  }

  public func createCustomHarness(_ harness: HarnessLaunch) async throws -> HarnessLaunch {
    let result = try await mutation(
      "harness.custom.create",
      HarnessCustomCreateParams(harness: harness, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomMutationResult.self)
    try await refreshAfterCustomHarnessMutation()
    return result.harness
  }

  public func updateCustomHarness(_ harness: HarnessLaunch) async throws -> HarnessLaunch {
    let result = try await mutation(
      "harness.custom.update",
      HarnessCustomUpdateParams(harness: harness, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomMutationResult.self)
    try await refreshAfterCustomHarnessMutation()
    return result.harness
  }

  public func removeCustomHarness(_ harnessId: String) async throws {
    _ = try await mutation(
      "harness.custom.remove",
      HarnessCustomRemoveParams(harnessId: harnessId, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomRemoveResult.self)
    try await refreshAfterCustomHarnessMutation()
  }

  public func refreshCatalogue(harness: String, paneId: String? = nil) {
    perform {
      var cursor: String?
      var models: [ModelInfo] = []
      var options: [ConfigOption] = []
      var source = ModelListSource.not_probed
      repeat {
        let page = try await self.request(
          "model.list",
          .encoded(
            ModelListParams(
              harness: harness, paneId: paneId, cursor: cursor, limit: 64))
        )
        .decoded(ModelListResult.self)
        models.append(contentsOf: page.items)
        options = page.options
        source = page.source
        cursor = page.nextCursor
      } while cursor != nil
      self.agentCatalogues[harness] = AgentCatalogue(
        models: models, options: options, source: source)
    }
  }

  public func createAgentPane(
    directory: URL, harness: String, model: String?, effort: String?, permission: PermissionMode
  ) {
    perform {
      _ = try await self.createAgentPaneNow(
        directory: directory, harness: harness, model: model, effort: effort,
        permission: permission)
    }
  }

  public func createAgentPaneNow(
    directory: URL, harness: String, model: String?, effort: String?, permission: PermissionMode
  ) async throws -> String {
    let path = directory.standardizedFileURL.path
    let workspaceId = try await workspaceId(for: directory, path: path)
    let result = try await mutation(
      "pane.create",
      PaneCreateParams(
        workspaceId: workspaceId, mode: .agent, cwd: path, name: "Agent",
        idempotencyKey: UUID().uuidString, harness: harness, model: model,
        effort: effort, permission: permission))
    let paneId = try result.decoded(PaneCreateResult.self).paneId
    select(paneId)
    return paneId
  }

  public func sendPrompt(_ paneId: String, text: String) {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    perform {
      let draft = self.agentWebDrafts[paneId]
      let prompt = draft.map { $0.markdown + "\n\n## Request\n\n" + text } ?? text
      guard prompt.utf8.count <= ProtocolLimits.promptBytes else {
        throw ClientFailure.invalidFrame
      }
      _ = try await self.mutation(
        "pane.prompt",
        PanePromptParams(
          paneId: paneId, text: prompt, origin: .human,
          attachments: draft?.images.map(\.reference),
          idempotencyKey: UUID().uuidString),
        attachments: draft?.images ?? [])
      self.agentWebDrafts.removeValue(forKey: paneId)
      try await self.loadAgentPane(paneId, replacing: false)
    }
  }

  public func attachWebElements(
    from sourcePaneId: String, package: WebElementPackage,
    images: [WebHostAttachment], to agentPaneId: String
  ) {
    let imageBytes = images.reduce(0) { $0 + $1.bytes.count }
    guard package.elements.count == images.count, !package.elements.isEmpty,
      package.elements.count <= 20,
      imageBytes
        <= (ProtocolLimits.acpMessageBytes - ProtocolLimits.promptBytes * 2 - 32_768) * 3 / 4,
      panes.contains(where: { $0.paneId == agentPaneId && $0.mode == .agent })
    else { return }
    agentWebDrafts[agentPaneId] = AgentWebAttachmentDraft(
      sourcePaneId: sourcePaneId, package: package, images: images)
    select(agentPaneId)
  }

  public func discardWebAttachmentDraft(_ paneId: String) {
    agentWebDrafts.removeValue(forKey: paneId)
  }

  public func cancelAgent(_ paneId: String) {
    perform {
      _ = try await self.mutation(
        "pane.cancel", PaneCancelParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }

  public func answerPermission(_ request: PermissionRequest, optionId: String) {
    perform {
      _ = try await self.mutation(
        "pane.permission.answer",
        PanePermissionAnswerParams(
          paneId: request.paneId, requestId: request.requestId, optionId: optionId,
          idempotencyKey: UUID().uuidString))
    }
  }

  public func answerElicitation(_ request: ElicitationRequest, fields: JsonValue?, cancel: Bool) {
    perform {
      _ = try await self.mutation(
        "pane.elicitation.answer",
        PaneElicitationAnswerParams(
          paneId: request.paneId, requestId: request.requestId, fields: fields,
          cancel: cancel, idempotencyKey: UUID().uuidString))
    }
  }

  public func setAgentConfig(_ paneId: String, configId: String, value: ConfigValue) {
    perform {
      _ = try await self.mutation(
        "pane.config.set",
        PaneConfigSetParams(
          paneId: paneId, configId: configId, value: value,
          idempotencyKey: UUID().uuidString),
        timeoutSeconds: 95)
    }
  }

  public func switchAgentHarness(
    _ paneId: String, harness: String, model: String? = nil, effort: String? = nil
  ) async throws -> PaneHarnessSwitchResult {
    let result = try await mutation(
      "pane.harness.switch",
      PaneHarnessSwitchParams(
        paneId: paneId, harness: harness, model: model, effort: effort,
        idempotencyKey: UUID().uuidString),
      timeoutSeconds: 95
    ).decoded(PaneHarnessSwitchResult.self)
    try await loadAgentPane(paneId, replacing: false)
    return result
  }

  public func setYolo(_ paneId: String, on: Bool) {
    perform {
      _ = try await self.mutation(
        "pane.yolo",
        PaneYoloParams(paneId: paneId, on: on, idempotencyKey: UUID().uuidString),
        timeoutSeconds: 95)
    }
  }

  public func setDraft(_ paneId: String, active: Bool) {
    perform {
      _ = try await self.mutation(
        "pane.draft",
        PaneDraftParams(
          paneId: paneId, active: active, idempotencyKey: UUID().uuidString))
    }
  }

  public func removeQueuedPrompt(_ paneId: String, promptId: String) {
    perform {
      _ = try await self.mutation(
        "pane.queue.remove",
        PaneQueueRemoveParams(
          paneId: paneId, promptId: promptId, idempotencyKey: UUID().uuidString))
    }
  }

  public func resumeAgentQueue(_ paneId: String) {
    perform {
      _ = try await self.mutation(
        "pane.queue.resume",
        PaneQueueResumeParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }

  public func startAgentAuthentication(_ paneId: String, methodId: String?) {
    guard !readOnly else {
      problem = "Finish agent sign-in on the Mac."
      return
    }
    perform {
      let result = try await self.mutation(
        "pane.auth.start",
        PaneAuthStartParams(
          paneId: paneId, authMethodId: methodId, idempotencyKey: UUID().uuidString)
      ).decoded(PaneAuthStartResult.self)
      if let terminal = result.terminalPaneId { self.select(terminal) }
    }
  }
}
