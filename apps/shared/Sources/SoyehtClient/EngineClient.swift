import Foundation
import Observation
import SoyehtProtocol

@MainActor @Observable
public final class EngineClient {
  public internal(set) var panes: [Pane] = []
  public internal(set) var workspaces: [Workspace] = []
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
  @ObservationIgnored var terminalOutputSink: ((Data, Bool) -> Void)?
  @ObservationIgnored var terminalOutputOwner: ObjectIdentifier?
  @ObservationIgnored var terminalOutputSinks: [String: TerminalOutputTarget] = [:]
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
  @ObservationIgnored var wire: Wire?
  #if DEBUG
    @ObservationIgnored var requestStub: RequestStub?
  #endif
  @ObservationIgnored var lifecycle: Task<Void, Never>?
  @ObservationIgnored var pending: [String: Pending] = [:]
  @ObservationIgnored var snapshot = SnapshotAssembly()
  /// Not observed: positions move with every chunk. A view reads `terminalGapTexts`.
  @ObservationIgnored var terminalStreams = TerminalStreamReplica() { didSet { syncGapTexts() } }
  var terminalGapTexts: [String: String] = [:]
  @ObservationIgnored var terminalSubscriptionsInFlight: Set<String> = []
  @ObservationIgnored var cursor: UInt64 = 0
  @ObservationIgnored var inputTask: Task<Void, Never>?
  @ObservationIgnored var inputQueue: [(String, Data)] = []
  @ObservationIgnored var queuedInputBytes = 0
  @ObservationIgnored var resizeTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var resizeGenerations: [String: UInt64] = [:]
  @ObservationIgnored var webHostTasks: [String: Task<Void, Never>] = [:]
  @ObservationIgnored var responseAttachments = ResponseAttachmentAssembly()
  /// Panes the engine closed since the last snapshot; a late event about one never brings it back.
  @ObservationIgnored var closedPaneIds: Set<String> = []
  @ObservationIgnored var lastAgentPaneByWorkspace: [String: String] = [:]
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
  @ObservationIgnored let socket: String
  @ObservationIgnored let credential: DeviceCredential?
  @ObservationIgnored let webHost: (any WebHostPort)?
  @ObservationIgnored var webHostRefused = false
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
  /// Runs `action` without waiting for it; a failure becomes the visible `problem`.
  public func perform(_ action: @escaping @MainActor () async throws -> Void) {
    Task { do { try await action() } catch { self.problem = error.localizedDescription } }
  }
}
