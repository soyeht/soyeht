import Foundation
import SoyehtProtocol

/// Frames in: responses, terminal chunks, the event dispatch and the state snapshot.
extension EngineClient {
  func receive(_ frame: Frame) throws {
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

  func applySnapshot(_ value: JsonValue) throws {
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
    try applyProjectTrustSnapshot(sections)
    applyWebSnapshot(panes: panes)
    pairRequests = requests
    cursor = UInt64(result.cursor)!
    if !panes.contains(where: { $0.paneId == selectedPaneId }) {
      selectedPaneId = result.focus?.paneId ?? panes.first?.paneId
    }
    snapshot = SnapshotAssembly()
  }
}
