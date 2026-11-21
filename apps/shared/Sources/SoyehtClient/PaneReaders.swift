import SoyehtProtocol

/// How a view reads the panes of the client, from the narrowest to the widest. Each reader runs
/// again only for the change it names, so a view reads the narrowest one that has what it draws.
extension EngineClient {
  /// The pane that is selected.
  public var selectedPane: Pane? { pane(selectedPaneId) }

  /// The pane `paneId`, followed on its own: a change in another pane does not run the reader.
  public func pane(_ paneId: String?) -> Pane? { paneStore.pane(paneId) }

  /// Where the pane `paneId` is: its workspace, mode and what its tab says. The reader runs when
  /// that changes, not at every turn of an agent.
  public func paneOutline(of paneId: String?) -> PaneOutline? { paneStore.outline(of: paneId) }

  /// The outlines of a workspace's panes, as its grid lays them out: the reader runs when a pane
  /// comes, goes, moves or is renamed, not at every turn of an agent. A view that draws a pane
  /// reads `pane(_:)`.
  public func outlinedPanes(in workspaceId: String) -> [PaneOutline] {
    paneStore.outlinedPanes(in: workspaceId)
  }

  /// The panes of a workspace, followed on their own: a change in another workspace's pane does
  /// not run the reader.
  public func panes(in workspaceId: String) -> [Pane] { paneStore.panes(in: workspaceId) }

  /// The panes of one mode, followed on their own.
  public func panes(mode: PaneMode) -> [Pane] { paneStore.panes(mode: mode) }

  /// The panes a workspace lays out: every pane except the editor panes behind
  /// the active tab, which share its slot.
  public func layoutPanes<Source: PaneLayoutSource>(_ workspacePanes: [Source]) -> [Source] {
    guard let workspaceId = workspacePanes.first(where: { $0.mode == .editor })?.workspaceId,
      let active = activeEditorPaneId(in: workspaceId)
    else { return workspacePanes }
    return workspacePanes.filter { $0.mode != .editor || $0.paneId == active }
  }
}
