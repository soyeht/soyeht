import SoyehtClient
import SwiftUI

/// The gestures that resize the panes of one split. The plain split and the writing companion's
/// split handle a divider the same way; they differ only in how a resized tree is computed.
@MainActor struct PaneDividerEditing {
  let client: EngineClient
  let workspaceId: String
  let interaction: PaneLayoutInteractionState
  let splitPath: PaneLayoutNodePath

  /// Resizes the panes beside the divider that follows child `afterChildIndex`. `resized` turns
  /// the tree the drag started from into the one to show.
  func resize(afterChildIndex: Int, resized: (PaneLayoutTree) throws -> PaneLayoutTree) {
    do {
      let active: PaneDividerGesture
      if let gesture = interaction.divider {
        active = gesture
      } else {
        let base = try client.paneLayouts.beginDraft(workspaceId: workspaceId)
        let gesture = PaneDividerGesture(
          workspaceId: workspaceId, splitPath: splitPath,
          afterChildIndex: afterChildIndex, baseTree: base)
        interaction.beginDivider(gesture)
        active = gesture
      }
      guard active.workspaceId == workspaceId, active.splitPath == splitPath,
        active.afterChildIndex == afterChildIndex
      else { return }
      try client.paneLayouts.updateDraft(
        workspaceId: workspaceId, tree: try resized(active.baseTree))
    } catch {
      cancel(message: "Couldn't resize the panes. The layout was not changed.")
    }
  }

  /// Commits the drag of the divider after child `afterChildIndex`.
  func finish(afterChildIndex: Int) {
    guard let active = interaction.divider,
      active.workspaceId == workspaceId, active.splitPath == splitPath,
      active.afterChildIndex == afterChildIndex
    else {
      interaction.clear()
      return
    }
    defer { interaction.clear() }
    do {
      guard client.paneLayouts.presentation(for: workspaceId)?.draftTree != nil else { return }
      try client.paneLayouts.commitDraft(workspaceId: workspaceId)
    } catch {
      client.paneLayouts.cancelDraft(workspaceId: workspaceId)
      client.problem = "Couldn't resize the panes. The layout was not changed."
    }
  }

  /// Gives the panes of the split equal shares; `equalized` turns the shown tree into the new one.
  func equalize(equalized: (PaneLayoutTree) throws -> PaneLayoutTree) {
    guard !interaction.blocksLayoutActions,
      let tree = client.paneLayouts.presentation(for: workspaceId)?.displayed.tree
    else { return }
    do {
      try client.paneLayouts.commit(tree: try equalized(tree), workspaceId: workspaceId)
    } catch {
      client.problem = "Couldn't equalize the panes. The layout was not changed."
    }
  }

  /// Drops a drag of this split that the layout has outgrown.
  func cancelIfActive() {
    guard interaction.divider?.workspaceId == workspaceId,
      interaction.divider?.splitPath == splitPath
    else { return }
    client.paneLayouts.cancelDraft(workspaceId: workspaceId)
    interaction.clear()
  }

  private func cancel(message: String) {
    client.paneLayouts.cancelDraft(workspaceId: workspaceId)
    interaction.clear()
    client.problem = message
  }
}
