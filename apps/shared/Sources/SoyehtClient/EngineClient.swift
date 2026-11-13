import Foundation
import Observation
import SoyehtProtocol

@MainActor @Observable
public final class EngineClient {
  public internal(set) var panes: [Pane] = []
  public private(set) var workspaces: [Workspace] = []
  public internal(set) var devices: [Device] = []
  public internal(set) var pairRequests: [PairPrompt] = []
  public internal(set) var agentPanes: [String: AgentPaneView] = [:]
  public internal(set) var agentWebDrafts: [String: AgentWebAttachmentDraft] = [:]
  public internal(set) var harnesses: [HarnessInfo] = []
  public internal(set) var harnessLoadCount = 0
  /// What the engine last said this machine has of what the app needs; `nil` until it does.
  public internal(set) var readiness: EngineReadinessResult?
  public internal(set) var customHarnesses: [HarnessLaunch] = []
  public internal(set) var harnessEnvironmentIssue: HarnessEnvironmentIssue?
  public internal(set) var preferredHarnessId: String?
  /// Default permission mode of new panes per harness; a key means asked (`nil`: no preference).
  public internal(set) var harnessDefaultModes: [String: String?] = [:]
  public internal(set) var agentCatalogues: [String: AgentCatalogue] = [:]
  public internal(set) var agentCatalogueLoadCounts: [String: Int] = [:]
  public internal(set) var tasks: [TaskRecord] = []
  public internal(set) var taskDetails: [String: TaskShowResult] = [:]
  public internal(set) var attentionByWorkspace: [String: [AttentionItem]] = [:]
  public internal(set) var paneLoops: [String: LoopConfig] = [:]
  /// The usage readings of the providers; the client asks the engine, the replica keeps them.
  public let usage = UsageReplica()
  /// The trust of each open workspace's folder; the client asks the engine, the replica keeps it.
  public let projectTrust = ProjectTrustReplica()
  public internal(set) var slashSources = SlashSources()
  public internal(set) var editorFiles: [String: EditorFileView] = [:]
  public internal(set) var editorNavigationTarget: EditorNavigationTarget?
  public internal(set) var editorWorkspace = EditorWorkspaceReplica()
  public internal(set) var writingDocuments: [String: WritingDocumentView] = [:]
  public internal(set) var webPanes: [String: WebPaneView] = [:]
  public internal(set) var localServers: [LocalServer] = []
  public internal(set) var connected = false
  public internal(set) var connectionRevision: UInt64 = 0
  public private(set) var credentialRejected = false
  public internal(set) var connectionText = ActiveProfile.values.connectingText
  public var problem: String?
  var backgroundLoads: [BackgroundLoad: BackgroundLoadRecord] = [:]
  public internal(set) var selectedPaneId: String?
  @ObservationIgnored lazy var terminalPresenters = TerminalPresenters(control: self)
  @ObservationIgnored public lazy var paneLayouts = PaneLayoutController {
    [weak self] workspaceId, state in
    guard let self else { throw ClientFailure.disconnected }
    let workspace = try await self.setWorkspacePaneLayout(workspaceId: workspaceId, state: state)
    return PaneLayoutState(
      workspaceLayout: workspace.layout,
      livePanes: self.layoutPanes(self.panes.filter { $0.workspaceId == workspaceId }))
  }
  public internal(set) var pairCode: DevicePairStartResult?
  /// Whether paired devices can reach the engine; `nil` until the engine says.
  public internal(set) var reachability: EngineReachability?
  public internal(set) var connectionActor: Actor?
  public internal(set) var engineCapabilities: Set<String> = []
  @ObservationIgnored private let terminalSinks = TerminalOutputSinks()
  #if os(macOS)
    @ObservationIgnored let terminalViewRegistry = MacTerminalViewRegistry()
  #endif
  public internal(set) var terminalOutput: ((Data, Bool) -> Void)? {
    get { terminalSinks.selected }
    set { terminalSinks.selected = newValue }
  }
  @ObservationIgnored private var wire: Wire?
  #if DEBUG
    @ObservationIgnored var requestStub: RequestStub?
  #endif
  @ObservationIgnored private var lifecycle: Task<Void, Never>?
  @ObservationIgnored private let pending = PendingRequests()
  @ObservationIgnored private var snapshot = SnapshotAssembly()
  /// Not observed: positions move with every chunk. A view reads `terminalGapTexts`.
  @ObservationIgnored var terminalStreams = TerminalStreamReplica() { didSet { syncGapTexts() } }
  var terminalGapTexts: [String: String] = [:]
  @ObservationIgnored private var terminalSubscriptionsInFlight: Set<String> = []
  @ObservationIgnored private var cursor: UInt64 = 0
  @ObservationIgnored private var inputTask: Task<Void, Never>?
  @ObservationIgnored private var inputQueue: [(String, Data)] = []
  @ObservationIgnored private var queuedInputBytes = 0
  @ObservationIgnored private var resizeTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var resizeGenerations: [String: UInt64] = [:]
  @ObservationIgnored private var webHostTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var responseAttachments = ResponseAttachmentAssembly()
  /// Panes the engine closed since the last snapshot; a late event about one never brings it back.
  @ObservationIgnored var closedPaneIds: Set<String> = []
  @ObservationIgnored private var lastAgentPaneByWorkspace: [String: String] = [:]
  @ObservationIgnored var agentTranscriptRevisions: [String: UInt64] = [:]
  @ObservationIgnored lazy var partials = AgentPartialFlusher(self)
  @ObservationIgnored var agentTranscriptLoadsInFlight: [String: UInt64] = [:]
  @ObservationIgnored var harnessRefreshGeneration: UInt64 = 0
  @ObservationIgnored var modelRefreshGenerations: [String: UInt64] = [:]
  @ObservationIgnored var editorWriteQueues: [String: [[TextEdit]]] = [:]
  @ObservationIgnored var editorWriteTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var editorRefreshTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var editorRefreshGeneration: [String: UInt64] = [:]
  @ObservationIgnored var writingRefreshTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var writingRefreshGeneration: [String: UInt64] = [:]
  @ObservationIgnored private let socket: String
  @ObservationIgnored private let credential: DeviceCredential?
  @ObservationIgnored private let webHost: (any WebHostPort)?
  @ObservationIgnored private var webHostRefused = false
  /// True for a paired iPhone or iPad. A paired device is a full remote control of the Mac;
  /// this only selects its transport, handshake and background behavior, never fewer powers.
  public var pairedDevice: Bool { credential != nil }
  /// Stable namespace for device-local presentation state. It is not a credential or token.
  public var engineId: String? { credential?.engineId }

  public init(socket: String = ActiveProfile.socketPath, webHost: (any WebHostPort)? = nil) {
    self.socket = socket
    credential = nil
    self.webHost = webHost
  }
  public init(credential: DeviceCredential) {
    self.credential = credential
    socket = ""
    webHost = nil
  }
  public var selectedPane: Pane? { panes.first { $0.paneId == selectedPaneId } }

  public func start() {
    guard lifecycle == nil else { return }
    lifecycle = Task { [weak self] in
      var backoff = ReconnectBackoff()
      while !Task.isCancelled {
        guard let self else { return }
        do {
          try await self.session()
          backoff.reset()
        } catch {
          self.connectionText = error.localizedDescription
          if case ClientFailure.remote(let kind, _, _) = error {
            // Another Mac app already hosts the browser: connect without claiming it.
            if kind == ErrorKind.web_host_unavailable.rawValue { self.webHostRefused = true }
            if self.pairedDevice
              && ClientFailure.refusesCredential(kind: kind)
            {
              self.credentialRejected = true
              self.disconnect()
              return
            }
          }
        }
        if Task.isCancelled { break }
        self.disconnect()
        do { try await Task.sleep(for: backoff.next()) } catch { break }
      }
    }
  }
  public func stop() {
    lifecycle?.cancel()
    lifecycle = nil
    disconnect()
  }
  private func disconnect() {
    (connected, engineCapabilities) = (false, [])
    resetAgentSession()
    for task in editorWriteTasks.values { task.cancel() }
    editorWriteTasks.removeAll()
    editorWriteQueues.removeAll()
    for task in editorRefreshTasks.values { task.cancel() }
    editorRefreshTasks.removeAll()
    editorRefreshGeneration.removeAll()
    for task in writingRefreshTasks.values { task.cancel() }
    writingRefreshTasks.removeAll()
    writingRefreshGeneration.removeAll()
    for replica in replicas { replica.cancelInFlight() }
    for task in webHostTasks.values { task.cancel() }
    webHostTasks.removeAll()
    for task in resizeTasks.values { task.cancel() }
    resizeTasks.removeAll()
    resizeGenerations.removeAll()
    terminalSubscriptionsInFlight.removeAll()
    if pairedDevice {
      panes = []
      workspaces = []
      paneLayouts.removeAll()
      agentPanes = [:]
      agentWebDrafts = [:]
      tasks = []
      taskDetails = [:]
      attentionByWorkspace = [:]
      paneLoops = [:]
      for replica in replicas { replica.reset() }
      editorFiles = [:]
      writingDocuments = [:]
      webPanes = [:]
      localServers = []
      terminalOutput?(Data(), true)
    }
    connectionActor = nil
    terminalStreams.removeAll()
    responseAttachments.reset()
    wire?.close()
    wire = nil
    pending.failAll(ClientFailure.disconnected)
    snapshot = SnapshotAssembly()
  }
  private func session() async throws {
    let wire = credential.map { Wire(url: $0.address) } ?? Wire(socket: socket)
    self.wire = wire
    try await wire.open()
    let challenge = try await wire.next()
    guard challenge.kind == 1 else { throw ClientFailure.invalidFrame }
    let first = try JSONDecoder().decode(JsonValue.self, from: challenge.payload)
    guard first["method"].eventName == .authChallenge else { throw ClientFailure.invalidFrame }
    let reader = Task { [weak self] in
      do {
        while !Task.isCancelled {
          let frame = try await wire.next()
          guard let self, self.wire === wire else { break }
          try self.receive(frame)
        }
      } catch {
        if let self, self.wire === wire {
          self.connectionText = error.localizedDescription
          self.disconnect()
        }
      }
    }
    defer {
      reader.cancel()
      wire.close()
    }
    let (hello, verification): (HelloParams, EngineVerification?) =
      try credential.map {
        try $0.hello(challenge: first["params"].decoded(AuthChallengeEvent.self))
      } ?? (.mac(capabilities: webHost == nil || webHostRefused ? nil : [.web_host]), nil)
    let welcome = try await request(.hello, .encoded(hello)).decoded(Welcome.self)
    try verification?.check(welcome)
    try ProtocolVersion.require(welcome.protocolVersion)
    guard welcome.role == (pairedDevice ? .human_device : .human_local) else {
      throw ClientFailure.invalidFrame
    }
    recordWelcome(welcome, client: hello.client, deviceId: credential?.deviceId)
    _ = try await request(.stateSnapshot, .object([:]))
    try await loadPreferredHarness()
    _ = try await request(.eventsSubscribe, .object(["since": .string(String(cursor))]))
    try await loadUsage()
    if !pairedDevice { try await loadLocalServers() }
    try await settleConnection()
    await reader.value
  }
  func request(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> JsonValue {
    try await requestResponse(
      method, params, timeoutSeconds: timeoutSeconds, attachments: attachments
    ).value
  }
  func requestResponse(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> EngineResponse {
    if let stubbed = try await stubbedResponse(method, params) { return stubbed }
    guard let wire else { throw ClientFailure.disconnected }
    return try await pendingResponse(
      method, params, timeoutSeconds: timeoutSeconds,
      waitForTimeout: PendingRequests.waitForTimeout
    ) { id in
      try await wire.send(
        .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "method": .string(method.rawValue), "params": params,
        ]), attachments: attachments)
    }
  }
  func pendingResponse(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    send: @escaping @MainActor (String) async throws -> Void
  ) async throws -> EngineResponse {
    try await pending.response(
      method, params, timeoutSeconds: timeoutSeconds, waitForTimeout: waitForTimeout,
      onTimeout: { [weak self] in self?.wire?.close() }, send: send)
  }
  /// Retry only connection loss, with the same mutation parameters and idempotency key.
  func mutation<T: Encodable>(
    _ method: RequestMethod, _ params: T, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> JsonValue {
    let value = try JsonValue.encoded(params)
    for attempt in 0..<3 {
      do {
        return try await request(
          method, value, timeoutSeconds: timeoutSeconds, attachments: attachments)
      } catch let error
        as ClientFailure
        where error.canRetry && attempt < 2
      {
        for _ in 0..<120 {
          if connected { break }
          try await Task.sleep(for: .milliseconds(100))
        }
      }
    }
    throw ClientFailure.disconnected
  }
  private func acceptWebHostRequest(_ message: JsonValue) throws -> Bool {
    guard let method = message["method"].string, let id = message["id"].string else {
      return false
    }
    guard method == RequestMethod.webCommand.rawValue, webHostTasks[id] == nil else {
      throw ClientFailure.invalidFrame
    }
    let command = try message["params"].decoded(WebCommandParams.self)
    guard webHostTasks.count < ProtocolLimits.webCommandWaiters else {
      throw ClientFailure.invalidFrame
    }
    let activeWire = wire
    let host = webHost
    webHostTasks[id] = Task { [weak self] in
      let response = await WebHostResponder.response(id: id, command: command, host: host)
      guard let self, self.wire === activeWire else { return }
      do { try await activeWire?.send(response) } catch { activeWire?.close() }
      self.webHostTasks.removeValue(forKey: id)
    }
    return true
  }
  private func receive(_ frame: Frame) throws {
    if frame.kind == 3 {
      try responseAttachments.accept(frame)
      return
    }
    if frame.kind == 2 {
      if let delivery = try terminalStreams.accept(frame) {
        deliverTerminalOutput(to: delivery.paneId, bytes: delivery.bytes, reset: false)
      }
      return
    }
    guard frame.kind == 1 else { return }
    let message = try JSONDecoder().decode(JsonValue.self, from: frame.payload)
    guard message["jsonrpc"].string == "2.0" else { throw ClientFailure.invalidFrame }
    if let id = message["id"].string, let request = pending.take(id) {
      request.timeout.cancel()
      if message["error"] != .null {
        if responseAttachments.isPending {
          responseAttachments.reset()
          request.continuation.resume(throwing: ClientFailure.invalidFrame)
          throw ClientFailure.invalidFrame
        }
        request.continuation.resume(
          throwing: ClientFailure.remote(
            message["error"]["data"]["kind"].string ?? ErrorKind.internal_error.rawValue,
            message["error"]["message"].string ?? "Request refused",
            retryAfterSec: message["error"]["data"]["retryAfterSec"].int.flatMap(UInt32.init)))
        return
      }
      let result = message["result"]
      do {
        let reference = try responseAttachmentReference(method: request.method, result: result)
        let attachment = try reference.map {
          WebHostAttachment(reference: $0, bytes: try responseAttachments.commit($0))
        }
        if reference == nil, responseAttachments.isPending { throw ClientFailure.invalidFrame }
        if request.method == .stateSnapshot { try applySnapshot(result) }
        if request.method == .paneStreamSubscribe,
          let paneId = request.params["paneId"].string
        {
          let info = try result.decoded(StreamInfo.self)
          // A close event can overtake an in-flight subscribe response. Do not resurrect stream
          // state after the pane (or its terminal role) has already disappeared.
          if panes.contains(where: { $0.paneId == paneId && $0.mode == .terminal }) {
            try terminalStreams.install(paneId: paneId, info: info)
          }
        }
        request.continuation.resume(
          returning: EngineResponse(value: result, attachment: attachment)
        )
      } catch {
        request.continuation.resume(throwing: error)
        throw error
      }
      return
    }
    if try acceptWebHostRequest(message) { return }
    if message["id"] == .null, message["error"] != .null {
      throw ClientFailure.remote(
        message["error"]["data"]["kind"].string ?? ErrorKind.internal_error.rawValue,
        message["error"]["message"].string ?? "Connection closed",
        retryAfterSec: message["error"]["data"]["retryAfterSec"].int.flatMap(UInt32.init))
    }
    guard let method = message["method"].string else { return }
    let params = message["params"]
    if let text = params["eventId"].string {
      guard let next = UInt64(text) else { throw ClientFailure.invalidFrame }
      if next <= cursor { return }
      cursor = next
    }
    guard let eventName = EventName(rawValue: method) else { return }
    switch eventName {
    case .stateSnapshotPart: try snapshot.append(params.decoded(StateSnapshotPartEvent.self))
    case .paneCreated, .paneUpdated, .paneRenamed, .paneClosed, .paneStateChanged:
      try applyPaneEvent(eventName, params)
    case .workspaceCreated, .workspaceRenamed, .workspaceLayoutUpdated:
      let workspace = try params["workspace"].decoded(Workspace.self)
      upsertWorkspace(workspace)
      synchronizePaneLayout(workspace)
    case .workspaceClosed: applyWorkspaceClosed(params)
    case .streamReplayStart:
      if let replay = try terminalStreams.start(params.decoded(StreamReplayStartEvent.self)),
        replay.reset
      {
        deliverTerminalOutput(to: replay.paneId, bytes: Data(), reset: true)
      }
    case .streamGap:
      _ = try terminalStreams.gap(params.decoded(StreamGapEvent.self))
    case .streamReplayDone:
      _ = try terminalStreams.finish(params.decoded(StreamReplayDoneEvent.self))
    case .devicePairRequested, .devicePaired, .deviceRevoked:
      try applyDeviceEvent(eventName, params)
    case .paneUpdate, .panePermissionRequested, .panePermissionAnswered,
      .paneElicitationRequested, .paneElicitationAnswered, .paneQueued,
      .paneQueuePaused, .paneQueueResumed, .paneSessionStarted, .paneTurnEnded,
      .paneSessionLost, .paneNeedsLogin, .paneAuthFinished, .paneConfigFailed,
      .paneHarnessSwitchStarted, .paneHarnessSwitchDone, .paneHarnessSwitchFailed:
      try applyAgentEvent(eventName, params)
    case .taskCreated, .taskMoved, .taskClaimed, .taskReleased, .taskCommented,
      .taskSubitemChanged, .taskBlocked, .taskUnblocked, .taskPhraseChanged,
      .taskUpdated, .taskAttention:
      try applyTaskEvent(eventName, params)
    case .attentionChanged: try applyAttentionEvent(params)
    case .loopTick, .loopStopped: try applyLoopEvent(eventName, params)
    case .usageChanged, .projectTrustChanged: try applyReplicaEvent(eventName, params)
    case .skillsChanged, .paneCommandsChanged: try applySlashEvent(eventName, params)
    case .fileChangedOnDisk, .fileSaved, .fileSaveFailed, .fileConflict, .lspDiagnostics,
      .editorAgentCursor, .editorReviewRequested, .editorTabsUpdated, .projectEntryRenamed,
      .projectTreeChanged:
      try applyEditorEvent(eventName, params)
    case .serversLocalChanged, .webNavigated, .webConsoleCounts, .webAgentDriving:
      try applyWebEvent(eventName, params)
    case .settingsChanged: try applySettingsChanged(params.decoded(SettingsChangedEvent.self))
    case .engineReachabilityChanged: try applyReachability(params)
    case .authChallenge, .pairResult, .engineUpdating, .engineRestarted,
      .ptydReconnected, .ptydLost, .paneConfigChanging, .paneConfigChanged, .projectTrustChanged:
      break
    case .writingChanged: try applyWritingEvent(params)
    }
  }
  func applyWorkspaceClosed(_ params: JsonValue) {
    let workspaceId = params["workspaceId"].string
    workspaces.removeAll { $0.workspaceId == workspaceId }
    forgetWorkspace(workspaceId)
  }
  func applyPaneEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .paneCreated:
      try applyPaneCreated(params.decoded(PaneCreatedEvent.self))
    case .paneUpdated, .paneRenamed:
      let pane = try params["pane"].decoded(Pane.self)
      acceptPane(pane)
    case .paneClosed: removePane(params["paneId"].string)
    case .paneStateChanged:
      let event = try params.decoded(PaneStateChangedEvent.self)
      if let index = panes.firstIndex(where: { $0.paneId == event.paneId }) {
        panes[index].state = event.state
        panes[index].source = event.source
        panes[index].confidence = event.confidence
        panes[index].phrase = event.phrase
        if panes[index].mode == .agent && event.state == .dead { refreshHarnesses() }
      }
    default: break
    }
  }

  func acceptPane(_ pane: Pane) {
    guard upsertPane(pane) else { return }
    #if os(macOS)
      if pane.mode == .terminal, connected, !terminalStreams.contains(pane.paneId) {
        perform(onPane: pane.paneId) { try await self.subscribe(pane.paneId) }
      }
    #endif
    if pane.mode == .agent {
      var view = agentPanes[pane.paneId] ?? AgentPaneView()
      view.queuePaused = pane.agent?.queuePaused ?? false
      view.authMethods = pane.agent?.authMethods ?? []
      agentPanes[pane.paneId] = view
    }
    upsertWebPane(pane)
  }

  func upsertWorkspace(_ workspace: Workspace) {
    if let index = workspaces.firstIndex(where: { $0.workspaceId == workspace.workspaceId }) {
      workspaces[index] = workspace
    } else {
      workspaces.append(workspace)
    }
  }
  private func applySnapshot(_ value: JsonValue) throws {
    let result = try value.decoded(StateSnapshotResult.self)
    let sections = try snapshot.finish(result)
    let panes = try (sections["panes"] ?? []).map { try $0.decoded(Pane.self) }
    let workspaces = try (sections["workspaces"] ?? []).map { try $0.decoded(Workspace.self) }
    let requests = try (sections["requests"] ?? []).filter { $0["kind"].string == "device_pair" }
      .map { try PairPrompt($0.decoded(DevicePairPending.self)) }
    self.devices = try (sections["devices"] ?? []).map { try $0.decoded(Device.self) }.filter {
      !$0.revoked
    }
    self.panes = panes
    closedPaneIds.removeAll()
    let terminalPaneIds = Set(panes.lazy.filter { $0.mode == .terminal }.map(\.paneId))
    terminalStreams.retainOnly(terminalPaneIds)
    terminalSinks.retainOnly(terminalPaneIds)
    for paneId in resizeTasks.keys.filter({ !terminalPaneIds.contains($0) }) {
      resizeTasks.removeValue(forKey: paneId)?.cancel()
      resizeGenerations.removeValue(forKey: paneId)
    }
    #if os(macOS)
      terminalViewRegistry.retainOnly(terminalPaneIds)
    #endif
    self.workspaces = workspaces
    paneLayouts.retainOnly(workspaceIds: Set(workspaces.map(\.workspaceId)))
    for workspace in workspaces { synchronizePaneLayout(workspace) }
    try applyAgentSnapshot(sections, panes: panes)
    try applyTaskSnapshot(sections, panes: panes)
    try applyEditorSnapshot(sections, panes: panes)
    try applyWritingSnapshot(sections, panes: panes)
    try applyProjectTrustSnapshot(sections)
    applyWebSnapshot(panes: panes)
    pairRequests = requests
    cursor = UInt64(result.cursor)!
    if !panes.contains(where: { $0.paneId == selectedPaneId }) {
      selectedPaneId = result.focus?.paneId ?? panes.first?.paneId
    }
    snapshot = SnapshotAssembly()
  }
  public func select(_ paneId: String?) {
    guard selectedPaneId != paneId else { return }
    let old = selectedPaneId
    selectedPaneId = paneId
    if let pane = panes.first(where: { $0.paneId == paneId }), pane.mode == .agent {
      lastAgentPaneByWorkspace[pane.workspaceId] = pane.paneId
    }
    #if !os(macOS)
      terminalOutput?(Data(), true)
    #endif
    guard connected else { return }
    perform {
      #if !os(macOS)
        if let old { try await self.unsubscribeAfterSelecting(from: old) }
      #endif
      if let paneId, self.selectedPaneId == paneId {
        guard try await self.focusEnginePane(paneId) else { return }
        if self.selectedPaneId == paneId,
          let pane = self.panes.first(where: { $0.paneId == paneId })
        {
          switch pane.mode {
          case .terminal:
            if !self.terminalStreams.contains(paneId) { try await self.subscribe(paneId) }
          case .agent:
            await self.hydratePresentedAgentPanes([
              PresentedPane(paneId: paneId, isVisible: true)
            ])
          case .editor: await self.loadEditorPane(paneId)
          case .web: break
          case .writing: try await self.loadWritingPane(paneId)
          }
        }
      }
    }
  }

  public func preferredAgentPane(in workspaceId: String) -> Pane? {
    if let paneId = lastAgentPaneByWorkspace[workspaceId],
      let pane = panes.first(where: { $0.paneId == paneId && $0.mode == .agent })
    {
      return pane
    }
    return panes.first { $0.workspaceId == workspaceId && $0.mode == .agent }
  }
  func subscribe(_ pane: String) async throws {
    guard !terminalSubscriptionsInFlight.contains(pane) else { return }
    terminalSubscriptionsInFlight.insert(pane)
    defer { terminalSubscriptionsInFlight.remove(pane) }
    _ = try await request(
      .paneStreamSubscribe, .object(["paneId": .string(pane), "from": .string("tail")]))
  }
  public func refreshTerminal() {
    guard let pane = selectedPaneId, selectedPane?.mode == .terminal, connected else { return }
    perform { try await self.subscribe(pane) }
  }
  public func refreshTerminal(_ paneId: String) {
    guard connected, panes.contains(where: { $0.paneId == paneId && $0.mode == .terminal })
    else { return }
    // macOS subscribes every terminal after the snapshot. A surface mounted after that must
    // consume the existing stream instead of replacing it and replaying the tail a second time.
    guard !terminalStreams.contains(paneId) else { return }
    perform { try await self.subscribe(paneId) }
  }
  public func gapText(for paneId: String) -> String? { terminalGapTexts[paneId] }
  func claimTerminalOutput(owner: AnyObject, sink: @escaping (Data, Bool) -> Void) {
    terminalSinks.claimSelected(owner: owner, sink: sink)
  }
  func releaseTerminalOutput(owner: AnyObject) { terminalSinks.releaseSelected(owner: owner) }
  func claimTerminalOutput(
    paneId: String, owner: AnyObject, sink: @escaping (Data, Bool) -> Void
  ) {
    terminalSinks.claim(paneId: paneId, owner: owner, sink: sink)
  }
  /// Whether `owner` held the pane's sink, which is now released.
  @discardableResult func releaseTerminalOutput(paneId: String, owner: AnyObject) -> Bool {
    terminalSinks.release(paneId: paneId, owner: owner)
  }
  func deliverTerminalOutput(to paneId: String, bytes: Data, reset: Bool) {
    terminalSinks.deliver(to: paneId, bytes: bytes, reset: reset, selectedPaneId: selectedPaneId)
  }
  var terminalOutputSinkCount: Int { terminalSinks.count }

  func discardTerminalPane(_ paneId: String) {
    terminalStreams.remove(paneId)
    terminalSubscriptionsInFlight.remove(paneId)
    terminalSinks.remove(paneId)
    resizeTasks.removeValue(forKey: paneId)?.cancel()
    resizeGenerations.removeValue(forKey: paneId)
    #if os(macOS)
      terminalViewRegistry.remove(paneId)
    #endif
  }
  public func setWorkspacePaneLayout(
    workspaceId: String, state: PaneLayoutState
  ) async throws -> Workspace {
    let result = try await mutation(
      .workspaceLayoutSet,
      WorkspaceLayoutSetParams(
        workspaceId: workspaceId, layout: try state.envelope(),
        idempotencyKey: UUID().uuidString))
    return try result.decoded(WorkspaceLayoutSetResult.self).workspace
  }
  public func closeWorkspace(_ workspaceId: String) async throws {
    _ = try await mutation(
      .workspaceClose,
      WorkspaceCloseParams(
        workspaceId: workspaceId, idempotencyKey: UUID().uuidString))
  }
  public func paneLayoutState(for workspace: Workspace) -> PaneLayoutState {
    PaneLayoutState(
      workspaceLayout: workspace.layout,
      livePanes: layoutPanes(panes.filter { $0.workspaceId == workspace.workspaceId }))
  }
  private func synchronizePaneLayout(_ workspace: Workspace) {
    paneLayouts.synchronize(
      workspaceId: workspace.workspaceId, confirmed: paneLayoutState(for: workspace))
  }
  public func closePane(_ id: String) {
    perform {
      _ = try await self.mutation(
        .paneClose, PaneCloseParams(paneId: id, idempotencyKey: UUID().uuidString))
    }
  }
  public func input(_ data: Data) {
    guard let pane = selectedPaneId else { return }
    input(data, paneId: pane)
  }
  func input(_ data: Data, paneId pane: String) {
    guard connected, selectedPaneId == pane,
      panes.contains(where: { $0.paneId == pane && $0.mode == .terminal }), !data.isEmpty
    else { return }
    guard queuedInputBytes + data.count <= inputQueueBytes else {
      problem = "Input is too large. Wait for the keyboard queue to empty."
      return
    }
    inputQueue.append((pane, data))
    queuedInputBytes += data.count
    guard inputTask == nil else { return }
    inputTask = Task {
      defer { inputTask = nil }
      while !inputQueue.isEmpty {
        let (pane, data) = inputQueue.removeFirst()
        queuedInputBytes -= data.count
        do {
          var offset = 0
          while offset < data.count {
            let chunk = data.subdata(in: offset..<min(offset + inputChunkBytes, data.count))
            let result = try await mutation(
              .paneInput,
              PaneInputParams(
                paneId: pane,
                bytes: chunk.base64EncodedString(), idempotencyKey: UUID().uuidString))
            guard let accepted = result["acceptedBytes"].int, accepted > 0, accepted <= chunk.count
            else {
              throw ClientFailure.invalidFrame
            }
            offset += accepted
          }
        } catch {
          problem = error.localizedDescription
          inputQueue.removeAll()
          queuedInputBytes = 0
          break
        }
      }
    }
  }
  public func resize(cols: Int, rows: Int) {
    guard let pane = selectedPaneId else { return }
    resize(paneId: pane, cols: cols, rows: rows)
  }
  func resize(paneId pane: String, cols: Int, rows: Int) {
    guard connected, panes.contains(where: { $0.paneId == pane && $0.mode == .terminal }),
      cols > 0, rows > 0
    else { return }
    resizeTasks[pane]?.cancel()
    let generation = resizeGenerations[pane, default: 0] &+ 1
    resizeGenerations[pane] = generation
    resizeTasks[pane] = Task {
      do {
        try await Task.sleep(for: .milliseconds(100))
        _ = try await mutation(
          .paneResize,
          PaneResizeParams(
            paneId: pane,
            cols: UInt32(min(cols, 1000)), rows: UInt32(min(rows, 1000)),
            idempotencyKey: UUID().uuidString))
      } catch is CancellationError {} catch { problem = error.localizedDescription }
      if resizeGenerations[pane] == generation {
        resizeTasks.removeValue(forKey: pane)
        resizeGenerations.removeValue(forKey: pane)
      }
    }
  }
  /// Runs `action` without waiting for it; a failure becomes the visible `problem`.
  public func perform(_ action: @escaping @MainActor () async throws -> Void) {
    Task { do { try await action() } catch { self.problem = error.localizedDescription } }
  }
}
