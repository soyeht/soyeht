import Foundation
import Observation
import SoyehtProtocol
import os

private let engineTimeoutLogger = Logger(
  subsystem: "com.soyeht.next", category: "engine-client-timeout")

struct EngineTimeoutDiagnostic: Equatable {
  let method: String
  let durationMs: UInt64
  let paneId: String?

  init(method: String, params: JsonValue, durationMs: UInt64) {
    self.method = method
    self.durationMs = durationMs
    self.paneId = params["paneId"].string.flatMap(UUID.init(uuidString:))?.uuidString.lowercased()
  }

  var logLine: String {
    "method=\(method) durationMs=\(durationMs) paneId=\(paneId ?? "none")"
  }
}

@MainActor @Observable
public final class EngineClient {
  public private(set) var panes: [Pane] = []
  public private(set) var workspaces: [Workspace] = []
  public private(set) var devices: [Device] = []
  public internal(set) var pairRequests: [PairPrompt] = []
  public internal(set) var agentPanes: [String: AgentPaneView] = [:]
  public internal(set) var agentWebDrafts: [String: AgentWebAttachmentDraft] = [:]
  public internal(set) var harnesses: [HarnessInfo] = []
  public internal(set) var harnessLoadCount = 0
  public internal(set) var customHarnesses: [HarnessLaunch] = []
  public internal(set) var harnessEnvironmentIssue: HarnessEnvironmentIssue?
  public internal(set) var preferredHarnessId: String?
  /// Preferred permission mode for new panes, per harness, as the harness names it.
  public internal(set) var harnessDefaultModes: [String: String] = [:]
  public internal(set) var agentCatalogues: [String: AgentCatalogue] = [:]
  public internal(set) var agentCatalogueLoadCounts: [String: Int] = [:]
  public internal(set) var tasks: [TaskRecord] = []
  public internal(set) var taskDetails: [String: TaskShowResult] = [:]
  public internal(set) var attentionByWorkspace: [String: [AttentionItem]] = [:]
  public internal(set) var paneLoops: [String: LoopConfig] = [:]
  public internal(set) var usageReadings: [UsageReading] = []
  public internal(set) var slashSources = SlashSources()
  public internal(set) var editorFiles: [String: EditorFileView] = [:]
  public internal(set) var editorNavigationTarget: EditorNavigationTarget?
  public internal(set) var editorWorkspace = EditorWorkspaceReplica()
  public internal(set) var writingDocuments: [String: WritingDocumentView] = [:]
  public internal(set) var webPanes: [String: WebPaneView] = [:]
  public internal(set) var localServers: [LocalServer] = []
  public private(set) var connected = false
  public private(set) var connectionRevision: UInt64 = 0
  public private(set) var credentialRejected = false
  public private(set) var connectionText = "Connecting to the Dev profile…"
  public var problem: String?
  public private(set) var selectedPaneId: String?
  @ObservationIgnored public lazy var paneLayouts = PaneLayoutController {
    [weak self] workspaceId, state in
    guard let self else { throw ClientFailure.disconnected }
    let workspace = try await self.setWorkspacePaneLayout(workspaceId: workspaceId, state: state)
    return PaneLayoutState(
      workspaceLayout: workspace.layout,
      livePanes: self.layoutPanes(self.panes.filter { $0.workspaceId == workspaceId }))
  }
  public var gapText: String? {
    selectedPaneId.flatMap { terminalStreams.gapText(for: $0) }
  }
  public internal(set) var pairCode: DevicePairStartResult?
  /// Whether paired devices can reach the engine; `nil` until the engine says.
  public internal(set) var reachability: EngineReachability?
  public private(set) var connectionActor: Actor?
  @ObservationIgnored private var terminalOutputSink: ((Data, Bool) -> Void)?
  @ObservationIgnored private var terminalOutputOwner: ObjectIdentifier?
  @ObservationIgnored private var terminalOutputSinks: [String: TerminalOutputTarget] = [:]
  #if os(macOS)
    @ObservationIgnored let terminalViewRegistry = MacTerminalViewRegistry()
  #endif
  public internal(set) var terminalOutput: ((Data, Bool) -> Void)? {
    get { terminalOutputSink }
    set {
      terminalOutputOwner = nil
      terminalOutputSink = newValue
    }
  }
  @ObservationIgnored private var wire: Wire?
  @ObservationIgnored private var lifecycle: Task<Void, Never>?
  @ObservationIgnored private var pending: [String: Pending] = [:]
  @ObservationIgnored private var snapshot = SnapshotAssembly()
  private var terminalStreams = TerminalStreamReplica()
  @ObservationIgnored private var terminalSubscriptionsInFlight: Set<String> = []
  @ObservationIgnored private var cursor: UInt64 = 0
  @ObservationIgnored private var inputTask: Task<Void, Never>?
  @ObservationIgnored private var inputQueue: [(String, Data)] = []
  @ObservationIgnored private var queuedInputBytes = 0
  @ObservationIgnored private var resizeTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var resizeGenerations: [String: UInt64] = [:]
  @ObservationIgnored private var webHostTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var responseAttachments = ResponseAttachmentAssembly()
  @ObservationIgnored private var lastAgentPaneByWorkspace: [String: String] = [:]
  @ObservationIgnored var agentTranscriptRevisions: [String: UInt64] = [:]
  @ObservationIgnored var agentTranscriptLoadsInFlight: [String: UInt64] = [:]
  @ObservationIgnored var harnessRefreshGeneration: UInt64 = 0
  @ObservationIgnored var modelRefreshGenerations: [String: UInt64] = [:]
  @ObservationIgnored var editorWriteQueues: [String: [[TextEdit]]] = [:]
  @ObservationIgnored var editorWriteTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var editorRefreshTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var editorRefreshGeneration: [String: UInt64] = [:]
  @ObservationIgnored var writingRefreshTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var writingRefreshGeneration: [String: UInt64] = [:]
  @ObservationIgnored var usageRefreshTasks: [String: Task<UsageRefreshResult, Error>] = [:]
  @ObservationIgnored private let socket: String
  @ObservationIgnored private let credential: DeviceCredential?
  @ObservationIgnored private let webHost: (any WebHostPort)?
  @ObservationIgnored private var webHostRefused = false
  /// True for a paired iPhone or iPad. A paired device is a full remote control of the Mac;
  /// this only selects its transport, handshake and background behavior, never fewer powers.
  public var pairedDevice: Bool { credential != nil }
  /// Stable namespace for device-local presentation state. It is not a credential or token.
  public var engineId: String? { credential?.engineId }
  public var harnessesLoading: Bool { harnessLoadCount > 0 }
  public func agentCatalogueLoading(_ harness: String) -> Bool {
    agentCatalogueLoadCounts[harness, default: 0] > 0
  }

  private struct Pending {
    var method: String
    var params: JsonValue
    var startedAtNs: UInt64
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>
  }

  private struct TerminalOutputTarget {
    var owner: ObjectIdentifier
    var sink: (Data, Bool) -> Void
  }

  struct EngineResponse: Sendable {
    var value: JsonValue
    var attachment: WebHostAttachment?
  }

  static func requestTimeoutDeadline(
    method: String, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (String, Double) async throws -> Void,
    onTimeout: @escaping @MainActor @Sendable (ClientFailure) -> Void
  ) -> Task<Void, Never> {
    Task {
      do { try await waitForTimeout(method, timeoutSeconds) } catch { return }
      onTimeout(.requestTimeout(method))
    }
  }

  private static func waitForRequestTimeout(_ method: String, _ seconds: Double) async throws {
    _ = method
    try await Task.sleep(for: .seconds(seconds))
  }

  public init(socket: String = DevProfile.socketPath, webHost: (any WebHostPort)? = nil) {
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
              && [ErrorKind.unauthorized.rawValue, ErrorKind.engine_identity_changed.rawValue]
                .contains(kind)
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
    connected = false
    agentTranscriptRevisions.removeAll()
    agentTranscriptLoadsInFlight.removeAll()
    for task in editorWriteTasks.values { task.cancel() }
    editorWriteTasks.removeAll()
    editorWriteQueues.removeAll()
    for task in editorRefreshTasks.values { task.cancel() }
    editorRefreshTasks.removeAll()
    editorRefreshGeneration.removeAll()
    for task in writingRefreshTasks.values { task.cancel() }
    writingRefreshTasks.removeAll()
    writingRefreshGeneration.removeAll()
    for task in usageRefreshTasks.values { task.cancel() }
    usageRefreshTasks.removeAll()
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
      usageReadings = []
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
    let requests = pending
    pending.removeAll()
    for request in requests.values {
      request.timeout.cancel()
      request.continuation.resume(throwing: ClientFailure.disconnected)
    }
    snapshot = SnapshotAssembly()
  }
  private func session() async throws {
    let wire = credential.map { Wire(url: $0.address) } ?? Wire(socket: socket)
    self.wire = wire
    try await wire.open()
    let challenge = try await wire.next()
    guard challenge.kind == 1 else { throw ClientFailure.invalidFrame }
    let first = try JSONDecoder().decode(JsonValue.self, from: challenge.payload)
    guard first["method"].string == "auth.challenge" else { throw ClientFailure.invalidFrame }
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
    let hello: HelloParams
    if let credential {
      hello = try credential.hello(challenge: first["params"].decoded(AuthChallengeEvent.self))
    } else {
      hello = HelloParams(
        client: "mac-dev", clientVersion: "0.1.0", protocolVersion: 1,
        capabilities: webHost == nil || webHostRefused ? nil : [.web_host])
    }
    let welcome = try await request("hello", .encoded(hello)).decoded(Welcome.self)
    guard welcome.role == (pairedDevice ? .human_device : .human_local),
      welcome.protocolVersion == 1
    else {
      throw ClientFailure.invalidFrame
    }
    connectionActor = Actor(
      role: welcome.role, client: hello.client, deviceId: credential?.deviceId)
    _ = try await request("state.snapshot", .object([:]))
    try await loadPreferredHarness()
    _ = try await request("events.subscribe", .object(["since": .string(String(cursor))]))
    try await loadUsage()
    if !pairedDevice { try await loadLocalServers() }
    connectionRevision &+= 1
    connected = true
    connectionText = "Dev profile · connected"
    if selectedPaneId == nil { selectedPaneId = panes.first?.paneId }
    #if os(macOS)
      // The tmux-style Mac surface keeps every terminal current, including hidden workspaces.
      for pane in panes where pane.mode == .terminal { try await subscribe(pane.paneId) }
    #endif
    if let pane = selectedPane {
      switch pane.mode {
      case .terminal:
        #if !os(macOS)
          // Phone still has one selected terminal surface; Block D will define its final policy.
          try await subscribe(pane.paneId)
        #endif
      case .agent:
        await hydratePresentedAgentPanes([PresentedPane(paneId: pane.paneId, isVisible: true)])
      case .editor: try await loadEditorPane(pane.paneId)
      case .web: break
      case .writing: try await loadWritingPane(pane.paneId)
      }
    }
    await reader.value
  }
  func request(
    _ method: String, _ params: JsonValue, timeoutSeconds: Double = 12,
    attachments: [WebHostAttachment] = []
  ) async throws -> JsonValue {
    try await requestResponse(
      method, params, timeoutSeconds: timeoutSeconds, attachments: attachments
    ).value
  }
  func requestResponse(
    _ method: String, _ params: JsonValue, timeoutSeconds: Double = 12,
    attachments: [WebHostAttachment] = []
  ) async throws -> EngineResponse {
    guard let wire else { throw ClientFailure.disconnected }
    return try await pendingResponse(
      method, params, timeoutSeconds: timeoutSeconds,
      waitForTimeout: Self.waitForRequestTimeout
    ) { id in
      try await wire.send(
        .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "method": .string(method), "params": params,
        ]), attachments: attachments)
    }
  }
  func pendingResponse(
    _ method: String, _ params: JsonValue, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (String, Double) async throws -> Void,
    send: @escaping @MainActor (String) async throws -> Void
  ) async throws -> EngineResponse {
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      let timeout = Self.requestTimeoutDeadline(
        method: method, timeoutSeconds: timeoutSeconds, waitForTimeout: waitForTimeout
      ) { [weak self] failure in
        guard let request = self?.pending.removeValue(forKey: id) else { return }
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - request.startedAtNs
        let diagnostic = EngineTimeoutDiagnostic(
          method: request.method, params: request.params, durationMs: elapsedNs / 1_000_000)
        engineTimeoutLogger.error("Engine RPC timeout: \(diagnostic.logLine, privacy: .public)")
        request.continuation.resume(throwing: failure)
        self?.wire?.close()
      }
      pending[id] = Pending(
        method: method, params: params, startedAtNs: DispatchTime.now().uptimeNanoseconds,
        continuation: continuation, timeout: timeout)
      Task {
        do {
          try await send(id)
        } catch {
          if let request = self.pending.removeValue(forKey: id) {
            request.timeout.cancel()
            request.continuation.resume(throwing: error)
          }
        }
      }
    }
  }
  /// Retry only connection loss, with the same mutation parameters and idempotency key.
  func mutation<T: Encodable>(
    _ method: String, _ params: T, timeoutSeconds: Double = 12,
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
    guard method == "web.command", webHostTasks[id] == nil else {
      throw ClientFailure.invalidFrame
    }
    let command = try message["params"].decoded(WebCommandParams.self)
    guard webHostTasks.count < ProtocolLimits.webCommandWaiters else {
      throw ClientFailure.invalidFrame
    }
    let activeWire = wire
    let host = webHost
    webHostTasks[id] = Task { [weak self] in
      let response = await Self.webHostResponse(id: id, command: command, host: host)
      guard let self, self.wire === activeWire else { return }
      do { try await activeWire?.send(response) } catch { activeWire?.close() }
      self.webHostTasks.removeValue(forKey: id)
    }
    return true
  }
  static func webHostResponse(
    id: String, command: WebCommandParams, host: (any WebHostPort)?
  ) async -> WebHostResponse {
    do {
      guard let host else {
        throw ClientFailure.webHost(.internal_error, "The web host is not ready.")
      }
      let output = try await host.execute(command)
      guard output.result.paneId == command.paneId else {
        throw ClientFailure.webHost(.internal_error, "The web host changed pane identity.")
      }
      return WebHostResponse(
        message: .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "result": try .encoded(output.result),
        ]), attachments: output.attachments)
    } catch let ClientFailure.webHost(kind, message) {
      return WebHostResponse(message: webHostError(id: id, kind: kind, message: message))
    } catch {
      return WebHostResponse(
        message: webHostError(id: id, kind: .internal_error, message: "The web command failed."))
    }
  }
  private static func webHostError(id: String, kind: ErrorKind, message: String) -> JsonValue {
    let safe: (ErrorKind, Int64) =
      switch kind {
      case .stale_reference: (.stale_reference, -32025)
      case .human_took_over: (.human_took_over, -32026)
      case .navigation_refused: (.navigation_refused, -32028)
      default: (.internal_error, -32018)
      }
    let (safeKind, code) = safe
    let error = RpcError(code: code, message: message, data: ErrorData(kind: safeKind))
    return .object([
      "jsonrpc": .string("2.0"), "id": .string(id),
      "error": (try? .encoded(error)) ?? .null,
    ])
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
    if let id = message["id"].string, let request = pending.removeValue(forKey: id) {
      request.timeout.cancel()
      if message["error"] != .null {
        if responseAttachments.isPending {
          responseAttachments.reset()
          request.continuation.resume(throwing: ClientFailure.invalidFrame)
          throw ClientFailure.invalidFrame
        }
        request.continuation.resume(
          throwing: ClientFailure.remote(
            message["error"]["data"]["kind"].string ?? "internal_error",
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
        if request.method == "state.snapshot" { try applySnapshot(result) }
        if request.method == "pane.stream.subscribe",
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
    case .workspaceClosed:
      applyWorkspaceClosed(params)
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
    case .usageChanged: try applyUsageEvent(params)
    case .skillsChanged, .paneCommandsChanged: try applySlashEvent(eventName, params)
    case .fileChangedOnDisk, .fileSaved, .fileConflict, .lspDiagnostics, .editorAgentCursor,
      .editorReviewRequested, .editorTabsUpdated, .projectEntryRenamed, .projectTreeChanged:
      try applyEditorEvent(eventName, params)
    case .serversLocalChanged, .webNavigated, .webConsoleCounts, .webAgentDriving:
      try applyWebEvent(eventName, params)
    case .settingsChanged: try applySettingsChanged(params.decoded(SettingsChangedEvent.self))
    case .engineReachabilityChanged: try applyReachability(params)
    case .authChallenge, .pairResult, .engineUpdating, .engineRestarted,
      .ptydReconnected, .ptydLost, .paneConfigChanging, .paneConfigChanged:
      break
    case .writingChanged: try applyWritingEvent(params)
    }
  }
  private func applyWorkspaceClosed(_ params: JsonValue) {
    let workspaceId = params["workspaceId"].string
    workspaces.removeAll { $0.workspaceId == workspaceId }
    if let workspaceId { paneLayouts.remove(workspaceId: workspaceId) }
  }
  private func applyPaneEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .paneCreated:
      try applyPaneCreated(params.decoded(PaneCreatedEvent.self))
    case .paneUpdated, .paneRenamed:
      let pane = try params["pane"].decoded(Pane.self)
      acceptPane(pane)
    case .paneClosed:
      let id = params["paneId"].string
      panes.removeAll { $0.paneId == id }
      if let id { discardTerminalPane(id) }
      if let id { agentPanes.removeValue(forKey: id) }
      if let id { agentWebDrafts.removeValue(forKey: id) }
      if let id { discardEditorPane(id) }
      if let id { discardWritingPane(id) }
      if let id { discardWebPane(id) }
      if selectedPaneId == id { select(panes.first?.paneId) }
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

  func applyPaneCreated(_ event: PaneCreatedEvent) throws {
    let pane = event.pane
    switch pane.mode {
    case .editor:
      if event.file == nil, event.writingDocument == nil { break }
      guard let file = event.file, event.writingDocument == nil,
        file.paneId == pane.paneId, file.workspaceId == pane.workspaceId
      else { throw ClientFailure.invalidFrame }
      upsertEditor(file)
    case .writing:
      if event.file == nil, event.writingDocument == nil { break }
      guard let file = event.file, let document = event.writingDocument,
        file.paneId == pane.paneId, file.workspaceId == pane.workspaceId,
        try JsonValue.encoded(file) == JsonValue.encoded(document.file),
        pane.writingKind == document.kind, pane.writingSection == document.section
      else { throw ClientFailure.invalidFrame }
      upsertEditor(file)
      writingDocuments[file.fileId] = WritingDocumentView(document: document)
    case .terminal, .agent, .web:
      guard event.file == nil, event.writingDocument == nil else {
        throw ClientFailure.invalidFrame
      }
    }
    acceptPane(pane)
  }

  private func acceptPane(_ pane: Pane) {
    upsertPane(pane)
    #if os(macOS)
      if pane.mode == .terminal, connected, !terminalStreams.contains(pane.paneId) {
        perform { try await self.subscribe(pane.paneId) }
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
  private func applyDeviceEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .devicePairRequested:
      let event = try params.decoded(DevicePairRequestedEvent.self)
      pairRequests.removeAll { $0.requestId == event.requestId }
      pairRequests.append(PairPrompt(event))
    case .devicePaired:
      let device = try params["device"].decoded(Device.self)
      pairRequests.removeAll { $0.fingerprint == device.fingerprint }
      devices.removeAll { $0.deviceId == device.deviceId }
      if !device.revoked { devices.append(device) }
    case .deviceRevoked: devices.removeAll { $0.deviceId == params["deviceId"].string }
    default: break
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
    let terminalPaneIds = Set(panes.lazy.filter { $0.mode == .terminal }.map(\.paneId))
    terminalStreams.retainOnly(terminalPaneIds)
    terminalOutputSinks = terminalOutputSinks.filter { terminalPaneIds.contains($0.key) }
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
        if let old, self.panes.first(where: { $0.paneId == old })?.mode == .terminal {
          _ = try await self.request(
            "pane.stream.unsubscribe", .object(["paneId": .string(old)]))
          self.terminalStreams.remove(old)
        }
      #endif
      if let paneId, self.selectedPaneId == paneId {
        _ = try await self.mutation(
          "pane.focus", PaneFocusParams(paneId: paneId, idempotencyKey: UUID().uuidString))
        if self.selectedPaneId == paneId,
          let pane = self.panes.first(where: { $0.paneId == paneId })
        {
          switch pane.mode {
          case .terminal:
            #if os(macOS)
              if !self.terminalStreams.contains(paneId) { try await self.subscribe(paneId) }
            #else
              try await self.subscribe(paneId)
            #endif
          case .agent:
            await self.hydratePresentedAgentPanes([
              PresentedPane(paneId: paneId, isVisible: true)
            ])
          case .editor: try await self.loadEditorPane(paneId)
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
  func upsertPane(_ pane: Pane) {
    if let index = panes.firstIndex(where: { $0.paneId == pane.paneId }) {
      panes[index] = pane
    } else {
      panes.append(pane)
    }
  }
  private func subscribe(_ pane: String) async throws {
    guard !terminalSubscriptionsInFlight.contains(pane) else { return }
    terminalSubscriptionsInFlight.insert(pane)
    defer { terminalSubscriptionsInFlight.remove(pane) }
    _ = try await request(
      "pane.stream.subscribe", .object(["paneId": .string(pane), "from": .string("tail")]))
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
  public func gapText(for paneId: String) -> String? { terminalStreams.gapText(for: paneId) }
  func claimTerminalOutput(
    owner: AnyObject, sink: @escaping (Data, Bool) -> Void
  ) {
    // SwiftUI may install a replacement before dismantling the prior surface.
    // The owner makes a late teardown unable to clear the replacement sink.
    terminalOutputOwner = ObjectIdentifier(owner)
    terminalOutputSink = sink
  }
  func releaseTerminalOutput(owner: AnyObject) {
    guard terminalOutputOwner == ObjectIdentifier(owner) else { return }
    terminalOutputOwner = nil
    terminalOutputSink = nil
  }
  func claimTerminalOutput(
    paneId: String, owner: AnyObject, sink: @escaping (Data, Bool) -> Void
  ) {
    terminalOutputSinks[paneId] = TerminalOutputTarget(
      owner: ObjectIdentifier(owner), sink: sink)
  }
  func releaseTerminalOutput(paneId: String, owner: AnyObject) {
    guard terminalOutputSinks[paneId]?.owner == ObjectIdentifier(owner) else { return }
    terminalOutputSinks.removeValue(forKey: paneId)
  }
  func deliverTerminalOutput(to paneId: String, bytes: Data, reset: Bool) {
    terminalOutputSinks[paneId]?.sink(bytes, reset)
    if paneId == selectedPaneId { terminalOutputSink?(bytes, reset) }
  }
  var terminalSubscriptionCount: Int { terminalStreams.count }
  var terminalOutputSinkCount: Int { terminalOutputSinks.count }

  private func discardTerminalPane(_ paneId: String) {
    terminalStreams.remove(paneId)
    terminalSubscriptionsInFlight.remove(paneId)
    terminalOutputSinks.removeValue(forKey: paneId)
    resizeTasks.removeValue(forKey: paneId)?.cancel()
    resizeGenerations.removeValue(forKey: paneId)
    #if os(macOS)
      terminalViewRegistry.remove(paneId)
    #endif
  }
  public func createPane(directory: URL) {
    perform { _ = try await self.createTerminalPane(directory: directory) }
  }
  public func createTerminalPane(directory: URL) async throws -> String {
    let path = directory.standardizedFileURL.path
    let workspaceId = try await workspaceId(for: directory, path: path)
    let result = try await mutation(
      "pane.create",
      PaneCreateParams(
        workspaceId: workspaceId,
        mode: .terminal, cwd: path, name: "Terminal", idempotencyKey: UUID().uuidString))
    let paneId = try result.decoded(PaneCreateResult.self).paneId
    select(paneId)
    return paneId
  }
  func workspaceId(for directory: URL, path: String) async throws -> String {
    if let existing = workspaces.first(where: { $0.path == path })?.workspaceId {
      return existing
    }
    let result = try await mutation(
      "workspace.create",
      WorkspaceCreateParams(
        name: directory.lastPathComponent, path: path, idempotencyKey: UUID().uuidString))
    return try result.decoded(WorkspaceCreateResult.self).workspace.workspaceId
  }
  public func setWorkspacePaneLayout(
    workspaceId: String, state: PaneLayoutState
  ) async throws -> Workspace {
    let result = try await mutation(
      "workspace.layout.set",
      WorkspaceLayoutSetParams(
        workspaceId: workspaceId, layout: try state.envelope(),
        idempotencyKey: UUID().uuidString))
    return try result.decoded(WorkspaceLayoutSetResult.self).workspace
  }
  public func closeWorkspace(_ workspaceId: String) async throws {
    _ = try await mutation(
      "workspace.close",
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
        "pane.close", PaneCloseParams(paneId: id, idempotencyKey: UUID().uuidString))
    }
  }
  public func setExperimentCapture(
    paneId: String, on: Bool
  ) async throws -> PaneExperimentSetResult {
    try await mutation(
      "pane.experiment.set",
      PaneExperimentSetParams(
        paneId: paneId, on: on, idempotencyKey: UUID().uuidString)
    ).decoded(PaneExperimentSetResult.self)
  }
  public func deleteExperimentCaptureData(
    paneId: String
  ) async throws -> PaneExperimentDeleteResult {
    try await mutation(
      "pane.experiment.delete",
      PaneExperimentDeleteParams(paneId: paneId, idempotencyKey: UUID().uuidString)
    ).decoded(PaneExperimentDeleteResult.self)
  }
  public func exportExperimentData(
    paneIds: [String], fromMs: String, toMs: String
  ) async throws -> PaneExperimentExportResult {
    try await mutation(
      "pane.experiment.export",
      PaneExperimentExportParams(
        paneIds: paneIds, fromMs: fromMs, toMs: toMs,
        idempotencyKey: UUID().uuidString)
    ).decoded(PaneExperimentExportResult.self)
  }
  public func input(_ data: Data) {
    guard let pane = selectedPaneId else { return }
    input(data, paneId: pane)
  }
  func input(_ data: Data, paneId pane: String) {
    guard connected, selectedPaneId == pane,
      panes.contains(where: { $0.paneId == pane && $0.mode == .terminal }), !data.isEmpty
    else { return }
    guard queuedInputBytes + data.count <= 1_048_576 else {
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
            let chunk = data.subdata(in: offset..<min(offset + 16_384, data.count))
            let result = try await mutation(
              "pane.input",
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
          "pane.resize",
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
  func perform(_ action: @escaping @MainActor () async throws -> Void) {
    Task { do { try await action() } catch { self.problem = error.localizedDescription } }
  }
}
