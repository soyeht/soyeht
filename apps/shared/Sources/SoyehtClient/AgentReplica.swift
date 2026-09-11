import Foundation
import SoyehtProtocol

public struct AgentPaneView: Sendable {
  public var messages: [TranscriptMessage] = []
  public var permission: PermissionRequest?
  public var elicitation: ElicitationRequest?
  public var queue: [PromptItem] = []
  public var queuePaused = false
  public var hasOlderMessages = false
  public var notice: String?
  public var authMethods: [AuthMethod] = []

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
}

public struct AgentCatalogue: Sendable {
  public var models: [ModelInfo]
  public var options: [ConfigOption]
  public var source: String

  public init(models: [ModelInfo] = [], options: [ConfigOption] = [], source: String = "not_probed")
  {
    self.models = models
    self.options = options
    self.source = source
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

  func applyAgentEvent(_ method: String, _ params: JsonValue) throws {
    switch method {
    case "pane.update":
      let update = try params.decoded(PaneUpdateEvent.self)
      let (paneId, message) = transcriptMessage(update)
      agentPanes[paneId, default: AgentPaneView()].merge(message)
    case "pane.permission_requested":
      let event = try params.decoded(PanePermissionRequestedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].permission = PermissionRequest(
        requestId: event.requestId, paneId: event.paneId, generation: event.generation,
        promptId: event.promptId, title: event.title, detail: event.detail,
        options: event.options, expiresAt: event.expiresAt)
    case "pane.permission_answered":
      let event = try params.decoded(PanePermissionAnsweredEvent.self)
      if agentPanes[event.paneId]?.permission?.requestId == event.requestId {
        agentPanes[event.paneId]?.permission = nil
      }
    case "pane.elicitation_requested":
      let event = try params.decoded(PaneElicitationRequestedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].elicitation = ElicitationRequest(
        requestId: event.requestId, paneId: event.paneId, generation: event.generation,
        promptId: event.promptId, title: event.title, schema: event.schema,
        expiresAt: event.expiresAt)
    case "pane.elicitation_answered":
      let event = try params.decoded(PaneElicitationAnsweredEvent.self)
      if agentPanes[event.paneId]?.elicitation?.requestId == event.requestId {
        agentPanes[event.paneId]?.elicitation = nil
      }
    case "pane.queued":
      let event = try params.decoded(PaneQueuedEvent.self)
      if event.paneId == selectedPaneId { refreshAgentQueue(event.paneId) }
    case "pane.queue.paused", "pane.queue.resumed":
      let event = try params.decoded(PaneQueueStateEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].queuePaused = event.paused
    case "pane.session_started":
      let event = try params.decoded(PaneSessionStartedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = nil
      refreshAgent(event.paneId, replacing: true)
    case "pane.turn_ended":
      let event = try params.decoded(PaneTurnEndedEvent.self)
      refreshAgent(event.paneId, replacing: false)
    case "pane.session_lost":
      let event = try params.decoded(PaneSessionLostEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
    case "pane.needs_login":
      let event = try params.decoded(PaneNeedsLoginEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].authMethods = event.authMethods
    case "pane.auth_finished":
      let event = try params.decoded(PaneAuthFinishedEvent.self)
      if event.ok {
        agentPanes[event.paneId, default: AgentPaneView()].authMethods = []
      } else {
        agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
      }
    case "pane.config_failed":
      let event = try params.decoded(PaneConfigFailedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
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
    if replacing { view.messages = [] }
    view.merge(result.items)
    view.hasOlderMessages = result.nextCursor != nil
    agentPanes[paneId] = view
    try await loadAgentQueue(paneId)
  }

  public func loadOlderAgentMessages(_ paneId: String) {
    guard let before = agentPanes[paneId]?.messages.first?.seq else { return }
    perform {
      let result = try await self.request(
        "pane.transcript",
        .encoded(PaneTranscriptParams(paneId: paneId, before: before, limit: 100))
      )
      .decoded(PaneTranscriptResult.self)
      var view = self.agentPanes[paneId] ?? AgentPaneView()
      view.merge(result.items)
      view.hasOlderMessages = result.nextCursor != nil
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

  public func refreshHarnesses() {
    perform {
      var cursor: String?
      var items: [HarnessInfo] = []
      repeat {
        let page = try await self.request(
          "harness.list", .encoded(HarnessListParams(cursor: cursor, limit: 64))
        )
        .decoded(HarnessListResult.self)
        items.append(contentsOf: page.items)
        cursor = page.nextCursor
      } while cursor != nil
      self.harnesses = items
    }
  }

  public func refreshCatalogue(harness: String, paneId: String? = nil) {
    perform {
      var cursor: String?
      var models: [ModelInfo] = []
      var options: [ConfigOption] = []
      var source = "not_probed"
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
      let path = directory.standardizedFileURL.path
      var workspaceId = self.workspaces.first { $0.path == path }?.workspaceId
      if workspaceId == nil {
        let result = try await self.mutation(
          "workspace.create",
          WorkspaceCreateParams(
            name: directory.lastPathComponent, path: path, idempotencyKey: UUID().uuidString))
        workspaceId = try result.decoded(WorkspaceCreateResult.self).workspace.workspaceId
      }
      guard let workspaceId else { throw ClientFailure.invalidFrame }
      let result = try await self.mutation(
        "pane.create",
        PaneCreateParams(
          workspaceId: workspaceId, mode: .agent, cwd: path, name: "Agent",
          idempotencyKey: UUID().uuidString, harness: harness, model: model,
          effort: effort, permission: permission))
      self.select(try result.decoded(PaneCreateResult.self).paneId)
    }
  }

  public func sendPrompt(_ paneId: String, text: String) {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    perform {
      _ = try await self.mutation(
        "pane.prompt",
        PanePromptParams(
          paneId: paneId, text: text, origin: .human, idempotencyKey: UUID().uuidString))
      try await self.loadAgentPane(paneId, replacing: false)
    }
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
