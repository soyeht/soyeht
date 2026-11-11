import Foundation
import SoyehtProtocol

/// What a split opens beside a pane.
public enum PaneSplitKind: Equatable, Sendable {
  /// A shell.
  case terminal
  /// An agent of the same harness as the pane (as the desktop does).
  case agent(harness: String)
  /// Neither: the person chooses (a writing pane's companion).
  case chooser
}

/// What the split buttons and keys of a pane header do (the Mac and the desktop alike). A split is
/// tmux's: it opens the new half at once, in the same workspace and the folder the source pane
/// is in, with no chooser in between: a shell beside a shell, an agent of the same harness
/// beside an agent. Choosing what a pane is stays with the "new pane" action.
public enum PaneSplitPolicy {
  /// What splitting `pane` opens. A writing pane splits into an agent companion of its document,
  /// which needs the chooser; any other pane that is not an agent opens a shell.
  public static func kind(splitting pane: Pane) -> PaneSplitKind {
    switch pane.mode {
    case .writing: .chooser
    case .agent: pane.agent.map { .agent(harness: $0.harness) } ?? .terminal
    case .terminal, .editor, .web: .terminal
    }
  }

  /// The folder the new pane starts in: the source pane's, else its workspace's.
  public static func directory(of anchor: Pane, workspacePath: String?) -> String? {
    if !anchor.cwd.isEmpty { return anchor.cwd }
    return workspacePath.flatMap { $0.isEmpty ? nil : $0 }
  }
}

extension EngineClient {
  /// Splits `anchorPaneId`: the pane `PaneSplitPolicy.kind` says opens beside it (`axis`
  /// horizontal puts it to the right, vertical below), in its workspace and folder, and takes the
  /// focus. The layout is computed from the one stored before the pane existed, the way the
  /// chooser completes a split, so both land the pane in the same place. Returns the new pane's
  /// identifier. A pane that needs the chooser is refused.
  @discardableResult
  public func split(
    _ anchorPaneId: String, axis: PaneLayoutAxis, sizing: PaneLayoutSizingPolicy
  ) async throws -> String {
    guard let anchor = panes.first(where: { $0.paneId == anchorPaneId }),
      let workspace = workspaces.first(where: { $0.workspaceId == anchor.workspaceId }),
      let directory = PaneSplitPolicy.directory(of: anchor, workspacePath: workspace.path)
    else { throw ClientFailure.invalidFrame }
    let kind = PaneSplitPolicy.kind(splitting: anchor)
    guard kind != .chooser else { throw ClientFailure.invalidFrame }
    let base = try paneLayouts.reconciledPresentation(
      for: workspace.workspaceId,
      livePanes: layoutPanes(panes.filter { $0.workspaceId == workspace.workspaceId }),
      sizing: sizing
    ).state.tree
    let folder = URL(fileURLWithPath: directory, isDirectory: true)
    let paneId: String
    let leafKind: PaneLayoutKind
    switch kind {
    case .agent(let harness):
      paneId = try await createAgentPaneNow(
        directory: folder, harness: harness, model: nil, effort: nil,
        workspaceId: workspace.workspaceId)
      leafKind = .agent
    case .terminal, .chooser:
      paneId = try await createTerminalPane(directory: folder, workspaceId: workspace.workspaceId)
      leafKind = .terminal
    }
    let leaf = PaneLayoutLeaf(paneId: paneId, kind: leafKind)
    let insertion = PaneLayoutProvisionalInsertion(
      baseTree: base, placeholder: leaf,
      placement: .split(anchorPaneId: anchorPaneId, axis: axis))
    try paneLayouts.commit(
      tree: insertion.completedTree(with: leaf, sizing: sizing),
      workspaceId: workspace.workspaceId)
    return paneId
  }
}
