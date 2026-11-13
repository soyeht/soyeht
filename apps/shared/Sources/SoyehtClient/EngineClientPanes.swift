import Foundation
import SoyehtProtocol

/// Pane selection and the pane events that change the pane list.
extension EngineClient {
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

}

/// Workspaces: the list, their pane layout and closing one.
extension EngineClient {
  func upsertWorkspace(_ workspace: Workspace) {
    if let index = workspaces.firstIndex(where: { $0.workspaceId == workspace.workspaceId }) {
      workspaces[index] = workspace
    } else {
      workspaces.append(workspace)
    }
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
  func synchronizePaneLayout(_ workspace: Workspace) {
    paneLayouts.synchronize(
      workspaceId: workspace.workspaceId, confirmed: paneLayoutState(for: workspace))
  }
  public func closePane(_ id: String) {
    perform {
      _ = try await self.mutation(
        .paneClose, PaneCloseParams(paneId: id, idempotencyKey: UUID().uuidString))
    }
  }
}
