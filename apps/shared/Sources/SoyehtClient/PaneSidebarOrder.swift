import SoyehtProtocol

/// One workspace and its panes, in the order the paired-device sidebar lists them.
public struct PaneSidebarSection: Identifiable, Sendable {
  public let workspace: Workspace
  public let panes: [Pane]
  public var id: String { workspace.workspaceId }

  /// Workspaces keep their replica order; each lists its panes in replica order.
  public static func sections(workspaces: [Workspace], panes: [Pane]) -> [PaneSidebarSection] {
    workspaces.map { workspace in
      PaneSidebarSection(
        workspace: workspace,
        panes: panes.filter { $0.workspaceId == workspace.workspaceId })
    }
  }
}

/// The pane identifiers of the sidebar, grouped by workspace. It decides which pane the
/// detail column shows when panes open or close and where keyboard navigation moves.
public struct PaneSidebarOrder: Equatable, Sendable {
  public let workspaces: [[String]]

  public init(workspaces: [[String]]) {
    self.workspaces = workspaces
  }

  public init(_ sections: [PaneSidebarSection]) {
    self.init(workspaces: sections.map { $0.panes.map(\.paneId) })
  }

  public var paneIds: [String] { workspaces.flatMap { $0 } }

  /// The selection after the pane list changed from `previous` to this order.
  ///
  /// A pane that still exists stays selected. When `fillsDetail` is false (compact width,
  /// where the list itself is the home screen) a closed pane clears the selection so the
  /// list returns. Otherwise a closed pane hands over to its nearest surviving neighbor,
  /// first in its own workspace, then anywhere; an empty selection takes `preferred` (the
  /// pane focused on the Mac) or the first pane.
  ///
  /// While the device is not connected the replica is empty because it was cleared, not because
  /// every pane closed, so the selection stays and the pane returns once the state does.
  public func reconciled(
    _ selection: String?, previous: PaneSidebarOrder, preferred: String?, fillsDetail: Bool,
    connected: Bool
  ) -> String? {
    guard connected else { return selection }
    let current = Set(paneIds)
    if let selection, current.contains(selection) { return selection }
    guard fillsDetail else { return nil }
    if let selection, let neighbor = previous.neighbor(of: selection, among: current) {
      return neighbor
    }
    if let preferred, current.contains(preferred) { return preferred }
    return paneIds.first
  }

  /// The pane `offset` steps away from `selection`, wrapping around the whole list.
  /// Without a selection, forward starts at the first pane and backward at the last.
  public func step(from selection: String?, by offset: Int) -> String? {
    let ids = paneIds
    guard !ids.isEmpty else { return nil }
    guard let selection, let index = ids.firstIndex(of: selection) else {
      return offset >= 0 ? ids.first : ids.last
    }
    let count = ids.count
    return ids[((index + offset) % count + count) % count]
  }

  /// The pane to show for the workspace at `index`: the current selection when it is
  /// already there, otherwise that workspace's first pane.
  public func pane(inWorkspaceAt index: Int, selection: String?) -> String? {
    guard workspaces.indices.contains(index) else { return nil }
    let panes = workspaces[index]
    if let selection, panes.contains(selection) { return selection }
    return panes.first
  }

  private func neighbor(of paneId: String, among current: Set<String>) -> String? {
    guard let workspace = workspaces.first(where: { $0.contains(paneId) }),
      let local = workspace.firstIndex(of: paneId),
      let global = paneIds.firstIndex(of: paneId)
    else { return nil }
    return Self.nearest(in: workspace, around: local, among: current)
      ?? Self.nearest(in: paneIds, around: global, among: current)
  }

  private static func nearest(in ids: [String], around index: Int, among current: Set<String>)
    -> String?
  {
    let after = ids[(index + 1)...].first(where: current.contains)
    return after ?? ids[..<index].last(where: current.contains)
  }
}
