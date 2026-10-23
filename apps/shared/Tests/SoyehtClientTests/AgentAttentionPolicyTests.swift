import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func agentPane(
  _ id: String, workspaceId: String, state: PaneState = .idle
) -> Pane {
  Pane(
    paneId: id, workspaceId: workspaceId, name: id, cwd: "/tmp", mode: .agent,
    state: state, source: .protocol, confidence: 1, phrase: "", generation: 1,
    cols: 80, rows: 24, record: true)
}

@Test func attentionAggregatesWorkingAndWaitingAcrossWorkspaces() {
  let panes = [
    agentPane("one", workspaceId: "left", state: .waiting_for_you),
    agentPane("two", workspaceId: "left", state: .working),
    agentPane("three", workspaceId: "right", state: .working),
    agentPane("four", workspaceId: "right"),
    Pane(
      paneId: "shell", workspaceId: "right", name: "shell", cwd: "/tmp",
      mode: .terminal, state: .waiting_for_you, source: .protocol, confidence: 1,
      phrase: "", generation: 1, cols: 80, rows: 24, record: false),
  ]
  var request = AgentPaneView()
  request.permission = PermissionRequest(
    requestId: "permission", paneId: "one", generation: 1, promptId: "prompt",
    title: "Write", detail: "", options: [], expiresAt: "later")
  var otherRequest = AgentPaneView()
  otherRequest.elicitation = ElicitationRequest(
    requestId: "question", paneId: "four", generation: 1, promptId: "prompt",
    title: "Question", schema: .object([:]), expiresAt: "later")
  let summary = AgentAttentionPolicy.summarize(
    panes: panes, agentPanes: ["one": request, "four": otherRequest])
  #expect(summary.totalWaitingCount == 2)
  #expect(summary.waitingCount(in: "left") == 1)
  #expect(summary.waitingCount(in: "right") == 1)
  #expect(summary.hasWorkingAgent)
  #expect(summary.hasWorkingAgent(in: "left"))
  #expect(summary.hasWorkingAgent(in: "right"))
}

@Test func attentionCountsPendingRequestsAndClearsThem() {
  let pane = agentPane("one", workspaceId: "left", state: .waiting_for_you)
  var view = AgentPaneView()
  view.permission = PermissionRequest(
    requestId: "permission", paneId: "one", generation: 1, promptId: "prompt",
    title: "Write", detail: "", options: [], expiresAt: "later")
  let first = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  #expect(first.waitingCount(in: "left") == 1)
  #expect(first.requests.map(\.identity) == ["one:1:permission"])

  view.elicitation = ElicitationRequest(
    requestId: "question", paneId: "one", generation: 1, promptId: "prompt",
    title: "Question", schema: .object([:]), expiresAt: "later")
  let second = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  #expect(second.waitingCount(in: "left") == 2)
  #expect(second.requests.map(\.requestId) == ["permission", "question"])

  view.permission = nil
  view.elicitation = nil
  let cleared = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  #expect(cleared.totalWaitingCount == 0)
  #expect(cleared.requests.isEmpty)
}

@Test func attentionLabelsUseRequestSingularAndPlural() {
  #expect(AgentAttentionPolicy.waitingLabel(count: 1) == "1 request waiting")
  #expect(AgentAttentionPolicy.waitingLabel(count: 2) == "2 requests waiting")
}

@Test func attentionSnapshotDoesNotRepeatExistingRequestAfterReconnect() {
  let pane = agentPane("one", workspaceId: "left")
  var view = AgentPaneView()
  view.permission = PermissionRequest(
    requestId: "permission", paneId: "one", generation: 1, promptId: "prompt",
    title: "Write", detail: "", options: [], expiresAt: "later")
  let before = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  let after = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  var cursor = AgentAttentionNotificationCursor()
  #expect(cursor.newRequests(in: before, connected: false).isEmpty)
  #expect(cursor.newRequests(in: before, connected: true).isEmpty)
  #expect(cursor.newRequests(in: after, connected: true).isEmpty)
  #expect(cursor.newRequests(in: after, connected: false).isEmpty)
  #expect(cursor.newRequests(in: after, connected: true).isEmpty)

  view.permission = PermissionRequest(
    requestId: "next", paneId: "one", generation: 1, promptId: "prompt",
    title: "Write", detail: "", options: [], expiresAt: "later")
  let next = AgentAttentionPolicy.summarize(panes: [pane], agentPanes: ["one": view])
  #expect(cursor.newRequests(in: next, connected: true).map(\.requestId) == ["next"])
  #expect(cursor.newRequests(in: next, connected: true).isEmpty)
}

@Test @MainActor func attentionNotificationsAreOnUntilTurnedOffAndNeedAnInactiveApp() throws {
  let preferences = MemoryPreferenceStore()
  let settings = AgentAttentionNotificationSettings(preferences: preferences)
  #expect(settings.enabled)
  #expect(settings.shouldPost(appIsActive: false))
  #expect(!settings.shouldPost(appIsActive: true))
  #expect(
    AgentAttentionNotificationSettings.enabledByDefault
      == (try ContractVector.load("agent-attention"))["enabledByDefault"].bool)

  settings.setEnabled(false)
  #expect(preferences.values[AgentAttentionNotificationSettings.enabledKey] == false)
  #expect(!settings.shouldPost(appIsActive: false))
  let restored = AgentAttentionNotificationSettings(preferences: preferences)
  #expect(!restored.enabled)
  settings.setEnabled(true)
  #expect(settings.shouldPost(appIsActive: false))
}

// MARK: - contracts/vectors/agent-attention.json

private func vectorSummary(_ panes: [JsonValue]) -> AgentAttentionSummary {
  var models: [Pane] = []
  var views: [String: AgentPaneView] = [:]
  for entry in panes {
    let id = entry["paneId"].string ?? ""
    let isAgent = entry["mode"].string == "agent"
    models.append(
      Pane(
        paneId: id, workspaceId: entry["workspaceId"].string ?? "",
        name: entry["name"].string ?? "", cwd: "/tmp", mode: isAgent ? .agent : .terminal,
        state: .idle, source: .protocol, confidence: 1, phrase: "", generation: 1, cols: 80,
        rows: 24, record: false))
    var view = AgentPaneView()
    if let requestId = entry["permission"]["requestId"].string {
      let permission = entry["permission"]
      view.permission = PermissionRequest(
        requestId: requestId, paneId: id,
        generation: UInt32(permission["generation"].int ?? 0), promptId: "prompt", title: "",
        detail: "", options: [], expiresAt: "later")
    }
    if let requestId = entry["elicitation"]["requestId"].string {
      let question = entry["elicitation"]
      view.elicitation = ElicitationRequest(
        requestId: requestId, paneId: id, generation: UInt32(question["generation"].int ?? 0),
        promptId: "prompt", title: "", schema: .object([:]), expiresAt: "later")
    }
    views[id] = view
  }
  return AgentAttentionPolicy.summarize(panes: models, agentPanes: views)
}

/// The request an identity (`paneId:generation:requestId`) names.
private func request(_ identity: String, workspaceId: String = "w") -> AgentAttentionRequest {
  let parts = identity.split(separator: ":").map(String.init)
  return AgentAttentionRequest(
    requestId: parts[2], paneId: parts[0], workspaceId: workspaceId, paneName: parts[0],
    harness: nil, generation: UInt32(parts[1]) ?? 0)
}

@Test func attentionRequestsAreTheOnesInTheSharedVector() throws {
  let vector = try ContractVector.load("agent-attention")
  for item in ContractVector.cases(vector, "requests") {
    let summary = vectorSummary(item["panes"].array ?? [])
    let name = item["name"].string ?? ""
    #expect(
      summary.requests.map(\.identity) == (item["identities"].array ?? []).compactMap(\.string),
      "\(name)")
    #expect(summary.totalWaitingCount == item["total"].int, "\(name)")
    guard case .object(let byWorkspace) = item["byWorkspace"] else { continue }
    for (workspace, count) in byWorkspace {
      #expect(summary.waitingCount(in: workspace) == count.int, "\(name) \(workspace)")
    }
  }
}

@Test func attentionCursorAnnouncesWhatTheSharedVectorSays() throws {
  let vector = try ContractVector.load("agent-attention")
  for item in ContractVector.cases(vector, "cursor") {
    var cursor = AgentAttentionNotificationCursor()
    for (index, step) in (item["steps"].array ?? []).enumerated() {
      let summary = AgentAttentionSummary(
        requests: (step["identities"].array ?? []).compactMap(\.string).map { request($0) },
        workingPaneIdsByWorkspace: [:])
      let new = cursor.newRequests(
        in: summary, connected: step["connected"].bool ?? false)
      #expect(
        new.map(\.identity) == (step["new"].array ?? []).compactMap(\.string),
        "\(item["name"].string ?? "") step \(index)")
    }
  }
}

@Test func attentionNotificationTextIsTheOneInTheSharedVector() throws {
  let notification = try ContractVector.load("agent-attention")["notification"]
  #expect(AgentAttentionPolicy.notificationTitle == notification["title"].string)
  for item in notification["bodies"].array ?? [] {
    #expect(
      AgentAttentionPolicy.notificationBody(
        paneName: item["paneName"].string ?? "", workspaceName: item["workspaceName"].string)
        == item["body"].string)
  }
}

@Test @MainActor func attentionDeliveryIsTheOneInTheSharedVector() throws {
  let vector = try ContractVector.load("agent-attention")
  for item in ContractVector.cases(vector, "delivery") {
    let settings = AgentAttentionNotificationSettings(preferences: MemoryPreferenceStore())
    settings.setEnabled(item["enabled"].bool ?? false)
    #expect(
      settings.shouldPost(appIsActive: item["appActive"].bool ?? false) == item["post"].bool)
  }
}

@Test func attentionRouteOpensOnlyWhileTheRequestIsPending() throws {
  let vector = try ContractVector.load("agent-attention")
  for item in ContractVector.cases(vector, "route") {
    let pending = item["pending"]
    let summary = AgentAttentionSummary(
      requests: (item["requests"].array ?? []).map {
        request($0["identity"].string ?? "", workspaceId: $0["workspaceId"].string ?? "")
      }, workingPaneIdsByWorkspace: [:])
    #expect(
      summary.contains(
        workspaceId: pending["workspaceId"].string ?? "", paneId: pending["paneId"].string ?? "",
        identity: pending["identity"].string ?? "") == item["openable"].bool,
      "\(item["name"].string ?? "")")
  }
}
