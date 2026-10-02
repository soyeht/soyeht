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

@Test @MainActor func attentionNotificationsRequireOptInAndInactiveApp() {
  let preferences = MemoryPreferenceStore()
  let settings = AgentAttentionNotificationSettings(preferences: preferences)
  #expect(!settings.enabled)
  #expect(!settings.shouldPost(appIsActive: false))

  settings.setEnabled(true)
  #expect(preferences.values[AgentAttentionNotificationSettings.enabledKey] == true)
  #expect(!settings.shouldPost(appIsActive: true))
  #expect(settings.shouldPost(appIsActive: false))

  let restored = AgentAttentionNotificationSettings(preferences: preferences)
  #expect(restored.enabled)
  settings.setEnabled(false)
  #expect(!settings.shouldPost(appIsActive: false))
}
