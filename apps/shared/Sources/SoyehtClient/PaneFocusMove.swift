import Foundation

/// A key that moves the focus between the panes of a workspace, as tmux's prefix arrows do.
public enum PaneFocusMove: Equatable, Sendable {
  case toward(PaneBoardDirection)
  case next
  case previous
}

extension EngineClient {
  /// The pane the focus moves to, from the selected one, on the board the workspace's panes
  /// make in `viewport` (the arrangement the person sees, tabs included). Nil when no pane
  /// lies that way, so the focus stays.
  public func paneToFocus(
    _ move: PaneFocusMove, workspaceId: String, viewport: PaneLayoutSize,
    sizing: PaneLayoutSizingPolicy
  ) -> String? {
    guard
      let board = paneBoard(workspaceId: workspaceId, viewport: viewport, sizing: sizing)
    else { return nil }
    switch move {
    case .toward(let direction):
      return board.neighbor(of: selectedPaneId, toward: direction, shown: nil)
    case .next:
      return board.focus(after: selectedPaneId, by: 1)
    case .previous:
      return board.focus(after: selectedPaneId, by: -1)
    }
  }
}
