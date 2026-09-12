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
  public internal(set) var harnesses: [HarnessInfo] = []
  public internal(set) var agentCatalogues: [String: AgentCatalogue] = [:]
  public internal(set) var tasks: [TaskRecord] = []
  public internal(set) var taskDetails: [String: TaskShowResult] = [:]
  public internal(set) var attentionByWorkspace: [String: [AttentionItem]] = [:]
  public internal(set) var paneLoops: [String: LoopConfig] = [:]
  public private(set) var connected = false
  public private(set) var connectionRevision: UInt64 = 0
  public private(set) var credentialRejected = false
  public private(set) var connectionText = "Connecting to the Dev profile…"
  public var problem: String?
  public private(set) var selectedPaneId: String?
  public private(set) var gapText: String?
  public private(set) var pairCode: DevicePairStartResult?
  @ObservationIgnored public var terminalOutput: ((Data, Bool) -> Void)?
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
  @ObservationIgnored private let socket: String
  @ObservationIgnored private let credential: DeviceCredential?
  public var readOnly: Bool { credential != nil }

  private struct Pending {
    var method: String
    var params: JsonValue
    var continuation: CheckedContinuation<JsonValue, Error>
    var timeout: Task<Void, Never>
  }

  public init(socket: String = DevProfile.socketPath) {
    self.socket = socket
    credential = nil
  }
  public init(credential: DeviceCredential) {
    self.credential = credential
    socket = ""
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
    if readOnly {
      panes = []
      workspaces = []
      agentPanes = [:]
      tasks = []
      taskDetails = [:]
      attentionByWorkspace = [:]
      paneLoops = [:]
      terminalOutput?(Data(), true)
    }
    position = nil
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
      hello = HelloParams(client: "mac-dev", clientVersion: "0.1.0", protocolVersion: 1)
    }
    let welcome = try await request("hello", .encoded(hello)).decoded(Welcome.self)
    guard welcome.role == (readOnly ? .human_device : .human_local), welcome.protocolVersion == 1
    else {
      throw ClientFailure.invalidFrame
    }
    _ = try await request("state.snapshot", .object([:]))
    _ = try await request("events.subscribe", .object(["since": .string(String(cursor))]))
    connectionRevision &+= 1
    connected = true
    connectionText = "Dev profile · connected"
    if selectedPaneId == nil { selectedPaneId = panes.first?.paneId }
    if let pane = selectedPane {
      if pane.mode == .terminal {
        try await subscribe(pane.paneId)
      } else {
        try await loadAgentPane(pane.paneId, replacing: true)
      }
    }
    await reader.value
  }
  func request(
    _ method: String, _ params: JsonValue, timeoutSeconds: Double = 12
  ) async throws -> JsonValue {
    guard let wire else { throw ClientFailure.disconnected }
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      let timeout = Task { [weak self] in
        do { try await Task.sleep(for: .seconds(timeoutSeconds)) } catch { return }
        guard let request = self?.pending.removeValue(forKey: id) else { return }
        request.continuation.resume(throwing: ClientFailure.timeout)
        self?.wire?.close()
      }
      pending[id] = Pending(
        method: method, params: params, continuation: continuation, timeout: timeout)
      Task {
        do {
          try await wire.send(
            .object([
              "jsonrpc": .string("2.0"), "id": .string(id),
              "method": .string(method), "params": params,
            ]))
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
    _ method: String, _ params: T, timeoutSeconds: Double = 12
  ) async throws -> JsonValue {
    let value = try JsonValue.encoded(params)
    for attempt in 0..<3 {
      do { return try await request(method, value, timeoutSeconds: timeoutSeconds) } catch let error
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
  private func receive(_ frame: Frame) throws {
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
        request.continuation.resume(
          throwing: ClientFailure.remote(
            message["error"]["data"]["kind"].string ?? "internal_error",
            "\(request.method): "
              + (message["error"]["message"].string ?? "Request refused")))
        return
      }
      let result = message["result"]
      do {
        if request.method == "state.snapshot" { try applySnapshot(result) }
        if request.method == "pane.stream.subscribe",
          request.params["paneId"].string == selectedPaneId
        {
          let info = try result.decoded(StreamInfo.self)
          position = StreamPosition(
            streamId: info.streamId, reference: info.binaryPaneRef,
            generation: info.generation, next: nil)
        }
        request.continuation.resume(returning: result)
      } catch {
        request.continuation.resume(throwing: error)
        throw error
      }
      return
    }
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
    case .paneCreated, .paneUpdated, .paneRenamed:
      let pane = try params["pane"].decoded(Pane.self)
      panes.removeAll { $0.paneId == pane.paneId }
      panes.append(pane)
      if pane.mode == .agent {
        var view = agentPanes[pane.paneId] ?? AgentPaneView()
        view.queuePaused = pane.agent?.queuePaused ?? false
        agentPanes[pane.paneId] = view
      }
    case .paneClosed:
      let id = params["paneId"].string
      panes.removeAll { $0.paneId == id }
      if let id { agentPanes.removeValue(forKey: id) }
      if selectedPaneId == id { select(panes.first?.paneId) }
    case .paneStateChanged:
      let event = try params.decoded(PaneStateChangedEvent.self)
      if let index = panes.firstIndex(where: { $0.paneId == event.paneId }) {
        panes[index].state = event.state
        panes[index].source = event.source
        panes[index].confidence = event.confidence
        panes[index].phrase = event.phrase
      }
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
      .paneSessionLost, .paneNeedsLogin, .paneAuthFinished, .paneConfigFailed:
      try applyAgentEvent(eventName, params)
    case .taskCreated, .taskMoved, .taskClaimed, .taskReleased, .taskCommented,
      .taskSubitemChanged, .taskBlocked, .taskUnblocked, .taskPhraseChanged,
      .taskUpdated, .taskAttention:
      try applyTaskEvent(eventName, params)
    case .attentionChanged: try applyAttentionEvent(params)
    case .loopTick, .loopStopped: try applyLoopEvent(eventName, params)
    case .authChallenge, .pairResult, .engineUpdating, .engineRestarted,
      .ptydReconnected, .ptydLost, .settingsChanged, .paneConfigChanging,
      .paneConfigChanged:
      break
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
          if pane.mode == .terminal {
            try await self.subscribe(paneId)
          } else {
            try await self.loadAgentPane(paneId, replacing: true)
          }
        }
      }
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
  public func createPane(directory: URL) {
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
          workspaceId: workspaceId,
          mode: .terminal, cwd: path, name: "Terminal", idempotencyKey: UUID().uuidString))
      self.select(try result.decoded(PaneCreateResult.self).paneId)
    }
  }
  public func closePane(_ id: String) {
    perform {
      _ = try await self.mutation(
        "pane.close", PaneCloseParams(paneId: id, idempotencyKey: UUID().uuidString))
    }
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
