import Foundation
import Observation
import SoyehtProtocol

@MainActor @Observable
public final class EngineClient {
  public private(set) var panes: [Pane] = []
  public private(set) var workspaces: [Workspace] = []
  public private(set) var devices: [Device] = []
  public private(set) var pairRequests: [PairPrompt] = []
  public internal(set) var agentPanes: [String: AgentPaneView] = [:]
  public internal(set) var agentWebDrafts: [String: AgentWebAttachmentDraft] = [:]
  public internal(set) var harnesses: [HarnessInfo] = []
  public internal(set) var harnessLoadCount = 0
  public internal(set) var customHarnesses: [HarnessLaunch] = []
  public internal(set) var harnessEnvironmentIssue: HarnessEnvironmentIssue?
  public internal(set) var agentCatalogues: [String: AgentCatalogue] = [:]
  public internal(set) var agentCatalogueLoadCounts: [String: Int] = [:]
  public internal(set) var tasks: [TaskRecord] = []
  public internal(set) var taskDetails: [String: TaskShowResult] = [:]
  public internal(set) var attentionByWorkspace: [String: [AttentionItem]] = [:]
  public internal(set) var paneLoops: [String: LoopConfig] = [:]
  public internal(set) var usageReadings: [UsageReading] = []
  public internal(set) var editorFiles: [String: EditorFileView] = [:]
  public internal(set) var editorNavigationTarget: EditorNavigationTarget?
  public internal(set) var writingDocuments: [String: WritingDocumentView] = [:]
  public internal(set) var webPanes: [String: WebPaneView] = [:]
  public internal(set) var localServers: [LocalServer] = []
  public private(set) var connected = false
  public private(set) var connectionRevision: UInt64 = 0
  public private(set) var credentialRejected = false
  public private(set) var connectionText = "Connecting to the Dev profile…"
  public var problem: String?
  public private(set) var selectedPaneId: String?
  public private(set) var gapText: String?
  public private(set) var pairCode: DevicePairStartResult?
  public private(set) var connectionActor: Actor?
  @ObservationIgnored private var terminalOutputSink: ((Data, Bool) -> Void)?
  @ObservationIgnored private var terminalOutputOwner: ObjectIdentifier?
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
  @ObservationIgnored private var position: StreamPosition?
  @ObservationIgnored private var cursor: UInt64 = 0
  @ObservationIgnored private var inputTask: Task<Void, Never>?
  @ObservationIgnored private var inputQueue: [(String, Data)] = []
  @ObservationIgnored private var queuedInputBytes = 0
  @ObservationIgnored private var resizeTask: Task<Void, Never>?
  @ObservationIgnored private var webHostTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored private var responseAttachments = ResponseAttachmentAssembly()
  @ObservationIgnored private var lastAgentPaneByWorkspace: [String: String] = [:]
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
  public var readOnly: Bool { credential != nil }
  public var harnessesLoading: Bool { harnessLoadCount > 0 }
  public func agentCatalogueLoading(_ harness: String) -> Bool {
    agentCatalogueLoadCounts[harness, default: 0] > 0
  }

  private struct Pending {
    var method: String
    var params: JsonValue
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>
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
      while !Task.isCancelled {
        guard let self else { return }
        do { try await self.session() } catch {
          self.connectionText = error.localizedDescription
          if case ClientFailure.remote(let kind, _) = error,
            self.readOnly
              && [ErrorKind.unauthorized.rawValue, ErrorKind.engine_identity_changed.rawValue]
                .contains(kind)
          {
            self.credentialRejected = true
            self.disconnect()
            return
          }
        }
        if Task.isCancelled { break }
        self.disconnect()
        do { try await Task.sleep(for: .seconds(1)) } catch { break }
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
    if readOnly {
      panes = []
      workspaces = []
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
    position = nil
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
        capabilities: webHost == nil ? nil : [.web_host])
    }
    let welcome = try await request("hello", .encoded(hello)).decoded(Welcome.self)
    guard welcome.role == (readOnly ? .human_device : .human_local), welcome.protocolVersion == 1
    else {
      throw ClientFailure.invalidFrame
    }
    connectionActor = Actor(
      role: welcome.role, client: hello.client, deviceId: credential?.deviceId)
    _ = try await request("state.snapshot", .object([:]))
    _ = try await request("events.subscribe", .object(["since": .string(String(cursor))]))
    try await loadUsage()
    if !readOnly { try await loadLocalServers() }
    connectionRevision &+= 1
    connected = true
    connectionText = "Dev profile · connected"
    if selectedPaneId == nil { selectedPaneId = panes.first?.paneId }
    if let pane = selectedPane {
      switch pane.mode {
      case .terminal: try await subscribe(pane.paneId)
      case .agent: try await loadAgentPane(pane.paneId, replacing: true)
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
        request.continuation.resume(throwing: failure)
        self?.wire?.close()
      }
      pending[id] = Pending(
        method: method, params: params, continuation: continuation, timeout: timeout)
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
      guard position != nil else { return }
      if let bytes = try position?.accept(frame) { terminalOutput?(bytes, false) }
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
            message["error"]["message"].string ?? "Request refused"))
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
          request.params["paneId"].string == selectedPaneId
        {
          let info = try result.decoded(StreamInfo.self)
          position = StreamPosition(
            streamId: info.streamId, reference: info.binaryPaneRef,
            generation: info.generation, next: nil)
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
        message["error"]["message"].string ?? "Connection closed")
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
    case .workspaceCreated, .workspaceRenamed:
      let workspace = try params["workspace"].decoded(Workspace.self)
      workspaces.removeAll { $0.workspaceId == workspace.workspaceId }
      workspaces.append(workspace)
    case .workspaceClosed: workspaces.removeAll { $0.workspaceId == params["workspaceId"].string }
    case .streamReplayStart:
      guard params["streamId"].string == position?.streamId else { return }
      if try position?.start(params.decoded(StreamReplayStartEvent.self)) == true {
        gapText = nil
        terminalOutput?(Data(), true)
      }
    case .streamGap:
      guard params["streamId"].string == position?.streamId else { return }
      try position?.gap(params.decoded(StreamGapEvent.self))
      gapText = "Some output was not recorded. The screen contains a gap."
    case .streamReplayDone:
      guard params["streamId"].string == position?.streamId else { return }
      guard params["nextOffset"].string == position?.next.map(String.init) else {
        throw ClientFailure.invalidStream
      }
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
    case .fileChangedOnDisk, .fileSaved, .fileConflict, .lspDiagnostics,
      .editorAgentCursor, .editorReviewRequested:
      try applyEditorEvent(eventName, params)
    case .serversLocalChanged, .webNavigated, .webConsoleCounts, .webAgentDriving:
      try applyWebEvent(eventName, params)
    case .authChallenge, .pairResult, .engineUpdating, .engineRestarted,
      .ptydReconnected, .ptydLost, .settingsChanged, .paneConfigChanging,
      .paneConfigChanged:
      break
    case .writingChanged: try applyWritingEvent(params)
    }
  }
  private func applyPaneEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .paneCreated, .paneUpdated, .paneRenamed:
      let pane = try params["pane"].decoded(Pane.self)
      upsertPane(pane)
      if pane.mode == .agent {
        var view = agentPanes[pane.paneId] ?? AgentPaneView()
        view.queuePaused = pane.agent?.queuePaused ?? false
        agentPanes[pane.paneId] = view
      }
      upsertWebPane(pane)
    case .paneClosed:
      let id = params["paneId"].string
      panes.removeAll { $0.paneId == id }
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
      }
    default: break
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
    self.workspaces = workspaces
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
    let old = selectedPaneId
    selectedPaneId = paneId
    if let pane = panes.first(where: { $0.paneId == paneId }), pane.mode == .agent {
      lastAgentPaneByWorkspace[pane.workspaceId] = pane.paneId
    }
    position = nil
    gapText = nil
    terminalOutput?(Data(), true)
    guard connected else { return }
    perform {
      if let old, self.panes.first(where: { $0.paneId == old })?.mode == .terminal {
        _ = try await self.request("pane.stream.unsubscribe", .object(["paneId": .string(old)]))
      }
      if let paneId, self.selectedPaneId == paneId {
        _ = try await self.mutation(
          "pane.focus", PaneFocusParams(paneId: paneId, idempotencyKey: UUID().uuidString))
        if self.selectedPaneId == paneId,
          let pane = self.panes.first(where: { $0.paneId == paneId })
        {
          switch pane.mode {
          case .terminal: try await self.subscribe(paneId)
          case .agent: try await self.loadAgentPane(paneId, replacing: true)
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
    _ = try await request(
      "pane.stream.subscribe", .object(["paneId": .string(pane), "from": .string("tail")]))
  }
  public func refreshTerminal() {
    guard let pane = selectedPaneId, selectedPane?.mode == .terminal, connected else { return }
    perform { try await self.subscribe(pane) }
  }
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
    guard !readOnly, connected, selectedPane?.mode == .terminal,
      let pane = selectedPaneId, !data.isEmpty
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
    guard connected, selectedPane?.mode == .terminal,
      let pane = selectedPaneId, cols > 0, rows > 0
    else { return }
    resizeTask?.cancel()
    resizeTask = Task {
      do {
        try await Task.sleep(for: .milliseconds(100))
        _ = try await mutation(
          "pane.resize",
          PaneResizeParams(
            paneId: pane,
            cols: UInt32(min(cols, 1000)), rows: UInt32(min(rows, 1000)),
            idempotencyKey: UUID().uuidString))
      } catch is CancellationError {} catch { problem = error.localizedDescription }
    }
  }
  public func beginPairing() {
    perform {
      self.pairCode = try await self.mutation(
        "device.pair.start",
        DevicePairStartParams(idempotencyKey: UUID().uuidString)
      ).decoded(DevicePairStartResult.self)
    }
  }
  public func revokeDevice(_ id: String) {
    perform {
      _ = try await self.mutation(
        "device.revoke", DeviceRevokeParams(deviceId: id, idempotencyKey: UUID().uuidString))
    }
  }
  public func clearPairCode() { pairCode = nil }
  public func decidePairing(_ request: PairPrompt, approve: Bool) {
    perform {
      _ = try await self.mutation(
        "device.pair.decide",
        DevicePairDecideParams(
          requestId: request.requestId, approve: approve, idempotencyKey: UUID().uuidString))
      self.pairRequests.removeAll { $0.requestId == request.requestId }
      if approve { self.pairCode = nil }
    }
  }
  func perform(_ action: @escaping @MainActor () async throws -> Void) {
    Task { do { try await action() } catch { self.problem = error.localizedDescription } }
  }
}

private func responseAttachmentReference(
  method: String, result: JsonValue
) throws -> AttachmentRef? {
  switch method {
  case "writing.asset.read", "web.screenshot":
    return try result["image"].decoded(AttachmentRef.self)
  case "web.snapshot" where result["screenshot"] != .null:
    return try result["screenshot"].decoded(AttachmentRef.self)
  default:
    return nil
  }
}
