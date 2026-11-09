import Foundation
import SoyehtProtocol

/// A workspace whose folder is not on the disk any more (deleted, moved, or on a disk that is not
/// there). The engine says so with `workspace_folder_missing`; the person sees a standing state of
/// the workspace, with the choice to close it, and never a dialog that returns at every launch.
/// There is no method to point a workspace at another folder, so closing it is the only choice.
public struct MissingWorkspaceFolder: Equatable, Sendable {
  public let workspaceId: String
  /// What the state says: the text of `ClientPolicy.workspaceFolderGone`, then the folder.
  public let text: String
  /// The label of the button that closes the workspace.
  public let closeTitle = PaneClosePolicy.closeWorkspaceTitle

  init(workspaceId: String, path: String) {
    self.workspaceId = workspaceId
    self.text = "\(ClientPolicy.workspaceFolderGone) \(path)"
  }
}

extension EngineClient {
  /// The state of `workspaceId` when its folder is gone, or nil when it is there (or not known yet).
  public func missingWorkspaceFolder(in workspaceId: String) -> MissingWorkspaceFolder? {
    guard editorWorkspace.missingFolders.contains(workspaceId),
      let workspace = workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return nil }
    return MissingWorkspaceFolder(workspaceId: workspaceId, path: workspace.path)
  }

  /// Marks the workspace when `error` is the engine saying its folder is gone. Returns whether it
  /// was, so the caller reports any other failure as it did before.
  @discardableResult
  func noteMissingWorkspaceFolder(_ error: Error, workspaceId: String) -> Bool {
    guard case ClientFailure.remote(let kind, _, _) = error,
      kind == ErrorKind.workspace_folder_missing.rawValue
    else { return false }
    editorWorkspace.missingFolders.insert(workspaceId)
    return true
  }
}
