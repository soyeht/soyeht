import SoyehtProtocol

/// Which web sessions a host makes and which it ends after the panes or the page shown change.
///
/// A host creates and ends sessions then, never while a view renders, so a view only reads the
/// session it is given. A session is made for a web pane that is on screen, and for the pane an
/// agent command names; a web pane in a workspace that is not shown costs nothing and loads
/// nothing.
public enum WebSessionLifecycle {
  /// The web panes that need a session: those of `shownWorkspaceId` when `destination` draws web
  /// panes. An agent command makes the session of the pane it names by itself.
  public static func needed(
    panes: [Pane], shownWorkspaceId: String?, destination: WorkspaceDestination
  ) -> Set<String> {
    guard destination.showsWebPanes else { return [] }
    return Set(
      panes.lazy.filter { $0.mode == .web && $0.workspaceId == shownWorkspaceId }.map(\.paneId))
  }

  /// The pane ids whose session no longer matches a web pane: the pane is gone or is no longer
  /// a web pane, or it switched between a private and a persistent session. `sessions` maps each
  /// live session's pane id to whether it is private.
  public static func ended(sessions: [String: Bool], panes: [Pane]) -> Set<String> {
    var current: [String: Bool] = [:]
    for pane in panes where pane.mode == .web { current[pane.paneId] = pane.incognito == true }
    return Set(sessions.filter { current[$0.key] != $0.value }.keys)
  }
}
