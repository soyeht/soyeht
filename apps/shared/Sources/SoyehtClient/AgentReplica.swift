import Foundation
import SoyehtProtocol

enum AgentCatalogResponseOrder {
  static func accepts(request: UInt64, latest: UInt64) -> Bool { request == latest }
}

public enum AgentPaneStatusPresentation {
  public static func title(
    state: PaneState, needsAuthentication: Bool, hasSession: Bool
  ) -> String {
    if needsAuthentication { return "Sign in" }
    if state == .starting && hasSession { return "ready" }
    return state.rawValue.replacingOccurrences(of: "_", with: " ")
  }
}

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
  /// Why the last sign-in did not finish; cleared when the next one starts or succeeds.
  public var signInFailure: String?
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

/// A surface reports which layout leaves are presented; the shared client decides
/// which agent transcripts need hydration without changing the engine's focus.
public struct PresentedPane: Equatable, Sendable {
  public let paneId: String
  public let isVisible: Bool

  public init(paneId: String, isVisible: Bool) {
    self.paneId = paneId
    self.isVisible = isVisible
  }
}

public enum AgentTranscriptHydrationPolicy {
  public static func paneIdsToLoad(
    presented: [PresentedPane], panes: [Pane], hydrated: Set<String>, loading: Set<String>
  ) -> [String] {
    let agents = Set(panes.filter { $0.mode == .agent }.map(\.paneId))
    var chosen = Set<String>()
    return presented.compactMap { leaf in
      guard leaf.isVisible, agents.contains(leaf.paneId), !hydrated.contains(leaf.paneId),
        !loading.contains(leaf.paneId), chosen.insert(leaf.paneId).inserted
      else { return nil }
      return leaf.paneId
    }
  }
}

/// When the agent transcript follows its newest message. Hidden tabs stay
/// mounted but do not scroll; they catch up when they become visible again.
public struct AgentConversationFollow: Equatable, Sendable {
  public static let bottomAnchor = "agent-bottom"

  /// Identifies what a reveal must react to: visibility and each pending
  /// request, so a new question fires even while a permission stays pending.
  public struct RevealKey: Equatable, Sendable {
    public let isVisible: Bool
    public let permissionId: String?
    public let elicitationId: String?

    public init(isVisible: Bool, permissionId: String?, elicitationId: String?) {
      self.isVisible = isVisible
      self.permissionId = permissionId
      self.elicitationId = elicitationId
    }

    var hasPendingRequest: Bool { permissionId != nil || elicitationId != nil }
  }

  public private(set) var hasUnseenMessages = false

  public init() {}

  /// Records a transcript change; returns whether to scroll to the bottom now.
  public mutating func messagesChanged(isVisible: Bool) -> Bool {
    if isVisible { return true }
    hasUnseenMessages = true
    return false
  }

  /// Whether a reveal must bring the newest messages and pending request into view.
  public func scrollsOnReveal(_ key: RevealKey) -> Bool {
    key.isVisible && (hasUnseenMessages || key.hasPendingRequest)
  }

  public mutating func revealed() {
    hasUnseenMessages = false
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
  /// Loads an agent transcript without changing shared pane focus. Phone companion
  /// tabs use this path because opening a local tab is not a remote focus action.
  public func loadAgentConversation(_ paneId: String) async throws {
    try await loadAgentPane(paneId, replacing: true)
  }

  public func hydratePresentedAgentPanes(_ presented: [PresentedPane]) async {
    guard connected else { return }
    let revision = connectionRevision
    let hydrated = Set(agentTranscriptRevisions.compactMap { $0.value == revision ? $0.key : nil })
    let loading = Set(
      agentTranscriptLoadsInFlight.compactMap {
        $0.value == revision ? $0.key : nil
      })
    let pending = AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: presented, panes: panes, hydrated: hydrated, loading: loading)
    for paneId in pending { agentTranscriptLoadsInFlight[paneId] = revision }
    for paneId in pending {
      defer {
        if agentTranscriptLoadsInFlight[paneId] == revision {
          agentTranscriptLoadsInFlight.removeValue(forKey: paneId)
        }
      }
      do {
        try await loadAgentPane(paneId, replacing: true)
      } catch {
        if absorbMissingPane(paneId, error) { continue }
        if connected && connectionRevision == revision { problem = error.localizedDescription }
      }
    }
  }

  func applyAgentSnapshot(_ sections: [String: [JsonValue]], panes: [Pane]) throws {
    let paneIds = Set(panes.filter { $0.mode == .agent }.map(\.paneId))
    agentPanes = agentPanes.filter { paneIds.contains($0.key) }
    agentTranscriptRevisions = agentTranscriptRevisions.filter { paneIds.contains($0.key) }
    agentTranscriptLoadsInFlight = agentTranscriptLoadsInFlight.filter { paneIds.contains($0.key) }
    for pane in panes where pane.mode == .agent {
      var view = agentPanes[pane.paneId] ?? AgentPaneView()
      view.permission = nil
      view.elicitation = nil
      view.queuePaused = pane.agent?.queuePaused ?? false
      view.authMethods = pane.agent?.authMethods ?? []
      view.harnessSwitch = pane.agent?.harnessSwitch.map {
        AgentHarnessSwitchView(
          fromHarness: $0.fromHarness, toHarness: $0.toHarness, phase: .switching)
      }
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
        presentation: event.presentation, options: event.options, expiresAt: event.expiresAt)
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
      refreshAgentQueue(event.paneId)
    case .paneQueuePaused, .paneQueueResumed:
      let event = try params.decoded(PaneQueueStateEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].queuePaused = event.paused
    case .paneSessionStarted:
      let event = try params.decoded(PaneSessionStartedEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = nil
      refreshAgent(event.paneId, replacing: true)
      refreshHarnesses()
    case .paneTurnEnded:
      let event = try params.decoded(PaneTurnEndedEvent.self)
      refreshAgent(event.paneId, replacing: false)
    case .paneSessionLost:
      let event = try params.decoded(PaneSessionLostEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].notice = event.reason
    case .paneNeedsLogin:
      let event = try params.decoded(PaneNeedsLoginEvent.self)
      agentPanes[event.paneId, default: AgentPaneView()].authMethods = event.authMethods
      refreshHarnesses()
    case .paneAuthFinished:
      let event = try params.decoded(PaneAuthFinishedEvent.self)
      var view = agentPanes[event.paneId, default: AgentPaneView()]
      if event.ok { view.authMethods = [] }
      view.signInFailure = event.ok ? nil : event.reason
      agentPanes[event.paneId] = view
      refreshHarnesses()
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
      refreshAgent(event.paneId, replacing: false)
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
          role: value.kind == .system_message ? .system : .assistant,
          kind: value.kind == .system_message ? .system : .message,
          text: value.text, complete: value.complete, subagentId: value.subagentId)
      )
    case .v1(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .thought, text: value.text, complete: value.complete,
          subagentId: value.subagentId)
      )
    case .v2(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .tool, text: "", complete: value.tool.status == .completed,
          tool: value.tool, subagentId: value.subagentId)
      )
    case .v3(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .tool, text: "",
          complete: [.completed, .failed].contains(value.tool.status), tool: value.tool,
          subagentId: value.subagentId)
      )
    case .v4(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .plan, text: "", complete: true, plan: value.entries,
          subagentId: value.subagentId)
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
    case .v8(let value):
      return (
        value.paneId,
        TranscriptMessage(
          localId: value.localId, generation: value.generation, seq: value.seq,
          role: .assistant, kind: .subagent, text: "",
          complete: value.subagent.status != .running, subagent: value.subagent,
          subagentId: value.subagentId)
      )
    }
  }

  func loadAgentPane(_ paneId: String, replacing: Bool) async throws {
    let revision = connectionRevision
    let result = try await request(
      .paneTranscript,
      .encoded(PaneTranscriptParams(paneId: paneId, limit: ClientPolicy.pageSizeTranscript))
    )
    .decoded(PaneTranscriptResult.self)
    guard connected && connectionRevision == revision,
      panes.contains(where: { $0.paneId == paneId && $0.mode == .agent })
    else { return }
    var view = agentPanes[paneId] ?? AgentPaneView()
    view.mergeTranscriptPage(result, replacing: replacing)
    agentPanes[paneId] = view
    try await loadAgentQueue(paneId)
    if connected && connectionRevision == revision { agentTranscriptRevisions[paneId] = revision }
  }

  func olderTranscriptParams(_ paneId: String) -> PaneTranscriptParams? {
    guard let before = agentPanes[paneId]?.nextTranscriptCursor else { return nil }
    return PaneTranscriptParams(
      paneId: paneId, before: before, limit: ClientPolicy.pageSizeTranscript)
  }

  public func loadOlderAgentMessages(_ paneId: String) {
    guard let params = olderTranscriptParams(paneId) else { return }
    perform(onPane: paneId) {
      let result = try await self.request(
        .paneTranscript,
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
        .paneQueueList,
        .encoded(
          PaneQueueListParams(paneId: paneId, cursor: cursor, limit: ClientPolicy.pageSizeQueue))
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
    perform(onPane: paneId) { try await self.loadAgentPane(paneId, replacing: replacing) }
  }

  func refreshAgentQueue(_ paneId: String) {
    perform(onPane: paneId) { try await self.loadAgentQueue(paneId) }
  }

  private func loadHarnesses(rescan: Bool) async throws {
    harnessRefreshGeneration &+= 1
    let generation = harnessRefreshGeneration
    harnessLoadCount += 1
    defer { harnessLoadCount -= 1 }
    var cursor: String?
    var items: [HarnessInfo] = []
    var environmentIssue: HarnessEnvironmentIssue?
    repeat {
      let page = try await request(
        .harnessList,
        .encoded(
          HarnessListParams(
            cursor: cursor, limit: ClientPolicy.pageSizeHarnesses,
            rescan: cursor == nil ? rescan : nil)),
        timeoutSeconds: Self.catalogueTimeoutSeconds
      )
      .decoded(HarnessListResult.self)
      items.append(contentsOf: page.items)
      environmentIssue = environmentIssue ?? page.environmentIssue
      cursor = page.nextCursor
    } while cursor != nil
    if AgentCatalogResponseOrder.accepts(request: generation, latest: harnessRefreshGeneration) {
      harnesses = items
      harnessEnvironmentIssue = environmentIssue
    }
  }

  public func refreshHarnesses(rescan: Bool = false) {
    performBackground(.harnesses) { try await self.loadHarnesses(rescan: rescan) }
  }

  private func loadCustomHarnesses() async throws {
    customHarnesses = try await request(
      .harnessCustomList, .encoded(HarnessCustomListParams())
    ).decoded(HarnessCustomListResult.self).items
  }

  public func refreshCustomHarnesses() {
    performBackground(.customHarnesses) { try await self.loadCustomHarnesses() }
  }

  private func refreshAfterCustomHarnessMutation() async throws {
    try await loadCustomHarnesses()
    try await loadHarnesses(rescan: true)
  }

  public func createCustomHarness(_ harness: HarnessLaunch) async throws -> HarnessLaunch {
    let result = try await mutation(
      .harnessCustomCreate,
      HarnessCustomCreateParams(harness: harness, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomMutationResult.self)
    try await refreshAfterCustomHarnessMutation()
    return result.harness
  }

  public func updateCustomHarness(_ harness: HarnessLaunch) async throws -> HarnessLaunch {
    let result = try await mutation(
      .harnessCustomUpdate,
      HarnessCustomUpdateParams(harness: harness, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomMutationResult.self)
    try await refreshAfterCustomHarnessMutation()
    return result.harness
  }

  public func removeCustomHarness(_ harnessId: String) async throws {
    _ = try await mutation(
      .harnessCustomRemove,
      HarnessCustomRemoveParams(harnessId: harnessId, idempotencyKey: UUID().uuidString)
    ).decoded(HarnessCustomRemoveResult.self)
    try await refreshAfterCustomHarnessMutation()
  }

  public func refreshCatalogue(harness: String, paneId: String? = nil) {
    performBackground(.catalogue(harness: harness)) {
      self.modelRefreshGenerations[harness, default: 0] &+= 1
      let generation = self.modelRefreshGenerations[harness, default: 0]
      self.agentCatalogueLoadCounts[harness, default: 0] += 1
      defer {
        let remaining = self.agentCatalogueLoadCounts[harness, default: 1] - 1
        if remaining == 0 {
          self.agentCatalogueLoadCounts.removeValue(forKey: harness)
        } else {
          self.agentCatalogueLoadCounts[harness] = remaining
        }
      }
      var cursor: String?
      var models: [ModelInfo] = []
      var options: [ConfigOption] = []
      var source = ModelListSource.not_probed
      repeat {
        let page = try await self.request(
          .modelList,
          .encoded(
            ModelListParams(
              harness: harness, paneId: paneId, cursor: cursor,
              limit: ClientPolicy.pageSizeModels)),
          timeoutSeconds: Self.catalogueTimeoutSeconds
        )
        .decoded(ModelListResult.self)
        models.append(contentsOf: page.items)
        options = page.options
        source = page.source
        cursor = page.nextCursor
      } while cursor != nil
      if AgentCatalogResponseOrder.accepts(
        request: generation, latest: self.modelRefreshGenerations[harness, default: 0]
      ) {
        self.agentCatalogues[harness] = AgentCatalogue(
          models: models, options: options, source: source)
      }
    }
  }

  /// `permissionMode` is one of the harness's own mode IDs; `nil` lets the
  /// engine apply the harness's preferred mode.
  public func createAgentPaneNow(
    directory: URL, harness: String, model: String?, effort: String?,
    permissionMode: String? = nil, selectPane: Bool = true
  ) async throws -> String {
    let path = directory.standardizedFileURL.path
    let workspaceId = try await workspaceId(for: directory, path: path)
    let result = try await mutation(
      .paneCreate,
      PaneCreateParams(
        workspaceId: workspaceId, mode: .agent, cwd: path,
        idempotencyKey: UUID().uuidString, harness: harness, model: model,
        effort: effort, permissionMode: permissionMode))
    let paneId = try result.decoded(PaneCreateResult.self).paneId
    if selectPane { select(paneId) }
    return paneId
  }

  public func sendPrompt(
    _ paneId: String, text: String, attachments: [AgentAttachmentDraft] = []
  ) {
    perform {
      try await self.sendPromptNow(paneId, text: text, attachments: attachments)
    }
  }

  public func sendPromptNow(
    _ paneId: String, text: String, attachments drafts: [AgentAttachmentDraft] = []
  ) async throws {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    let draft = agentWebDrafts[paneId]
    let prompt = draft.map { $0.markdown + "\n\n## Request\n\n" + text } ?? text
    guard prompt.utf8.count <= ProtocolLimits.promptBytes else {
      throw ClientFailure.invalidFrame
    }
    let (attachments, frames) = Self.promptAttachments(web: draft, drafts: drafts)
    _ = try await mutation(
      .panePrompt,
      PanePromptParams(
        paneId: paneId, text: prompt, origin: .human,
        attachments: attachments.isEmpty ? nil : attachments,
        idempotencyKey: UUID().uuidString),
      attachments: frames)
    agentWebDrafts.removeValue(forKey: paneId)
    try await loadAgentPane(paneId, replacing: false)
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
        .paneCancel, PaneCancelParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }

  public func answerPermission(_ request: PermissionRequest, optionId: String) {
    perform {
      try await self.answerPermission(
        paneId: request.paneId, requestId: request.requestId, optionId: optionId)
    }
  }

  public func answerElicitation(_ request: ElicitationRequest, fields: JsonValue?, cancel: Bool) {
    perform {
      _ = try await self.mutation(
        .paneElicitationAnswer,
        PaneElicitationAnswerParams(
          paneId: request.paneId, requestId: request.requestId, fields: fields,
          cancel: cancel, idempotencyKey: UUID().uuidString))
    }
  }

  public func setAgentConfig(_ paneId: String, configId: String, value: ConfigValue) {
    perform {
      _ = try await self.mutation(
        .paneConfigSet,
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
      .paneHarnessSwitch,
      PaneHarnessSwitchParams(
        paneId: paneId, harness: harness, model: model, effort: effort,
        idempotencyKey: UUID().uuidString),
      timeoutSeconds: 95
    ).decoded(PaneHarnessSwitchResult.self)
    try await loadAgentPane(paneId, replacing: false)
    return result
  }

  public func setDraft(_ paneId: String, active: Bool) {
    perform {
      _ = try await self.mutation(
        .paneDraft,
        PaneDraftParams(
          paneId: paneId, active: active, idempotencyKey: UUID().uuidString))
    }
  }

  public func removeQueuedPrompt(_ paneId: String, promptId: String) {
    perform {
      _ = try await self.mutation(
        .paneQueueRemove,
        PaneQueueRemoveParams(
          paneId: paneId, promptId: promptId, idempotencyKey: UUID().uuidString))
    }
  }

  public func resumeAgentQueue(_ paneId: String) {
    perform {
      _ = try await self.mutation(
        .paneQueueResume,
        PaneQueueResumeParams(paneId: paneId, idempotencyKey: UUID().uuidString))
    }
  }
}
