import SoyehtProtocol

/// A workspace's editor panes share one slot in the pane layout: the editor
/// area, which shows the active tab. Layout code receives only that pane among
/// the live panes, and a stored layout that still names another editor pane
/// (an earlier active tab, or a layout saved before tabs) hands its slot to it.
extension PaneLayoutTree {
  /// Gives the first editor leaf whose pane is no longer listed to the single
  /// live editor pane, when that pane has no leaf yet. Every other layout is
  /// returned unchanged; leaves of panes that are gone are removed elsewhere.
  public func adoptingLiveEditor(_ livePanes: [Pane]) -> PaneLayoutTree {
    let editors = livePanes.filter { $0.mode == .editor }
    guard editors.count == 1, let editor = editors.first,
      !paneIdsInCanonicalOrder.contains(editor.paneId)
    else { return self }
    let live = Set(livePanes.map(\.paneId))
    guard
      let slot = leavesInCanonicalOrder.first(where: {
        $0.kind == .editor && !live.contains($0.paneId)
      })
    else { return self }
    return replacingPane(slot.paneId, with: editor.paneId)
  }

  func replacingPane(_ paneId: String, with replacement: String) -> PaneLayoutTree {
    switch self {
    case .leaf(let leaf):
      guard leaf.paneId == paneId else { return self }
      return .leaf(PaneLayoutLeaf(paneId: replacement, kind: leaf.kind))
    case .split(var split):
      split.children = split.children.map {
        PaneLayoutBranch(weight: $0.weight, tree: $0.tree.replacingPane(paneId, with: replacement))
      }
      return .split(split)
    }
  }
}

extension EngineClient {
  /// The panes a workspace lays out: every pane except the editor panes behind
  /// the active tab, which share its slot.
  public func layoutPanes(_ workspacePanes: [Pane]) -> [Pane] {
    guard let workspaceId = workspacePanes.first(where: { $0.mode == .editor })?.workspaceId,
      let active = activeEditorPaneId(in: workspaceId)
    else { return workspacePanes }
    return workspacePanes.filter { $0.mode != .editor || $0.paneId == active }
  }
}
