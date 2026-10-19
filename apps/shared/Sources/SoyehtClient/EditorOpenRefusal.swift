import Foundation
import SoyehtProtocol

/// A file the engine would not open as text. Where a failure is shown follows what failed:
/// a file that cannot be shown is a state of the editor, with its reason on the pane where the
/// text would be, and the person has nothing to confirm; every other failure of an action stays
/// a dialog (`EngineClient.problem`), since something was lost or must be known.
///
/// The cases are `contracts/vectors/editor.json` (`openFailure`), which the desktop client
/// passes as well (`soyeht_client::editor::refusal`).
public struct EditorOpenRefusal: Equatable, Sendable {
  /// The workspace path that was asked for.
  public let path: String
  /// What the pane says in place of the text.
  public let text: String

  /// The refusal an open of `path` ended in, or nil when `error` is not one.
  public init?(error: Error, path: String) {
    guard case ClientFailure.remote(let kind, _, _) = error, let text = Self.text(kind: kind)
    else { return nil }
    self.path = path
    self.text = text
  }

  /// The pane text of an error kind the pane shows, nil for a kind that is a dialog.
  static func text(kind: String) -> String? {
    switch kind {
    case ErrorKind.file_not_text.rawValue: ClientPolicy.editorFileNotText
    case ErrorKind.file_too_large.rawValue: ClientPolicy.editorFileTooLarge
    default: nil
    }
  }
}

extension EngineClient {
  /// What the editor of a workspace says in place of the file the person last tried to open.
  public func editorOpenRefusal(in workspaceId: String) -> EditorOpenRefusal? {
    editorWorkspace.refusals[workspaceId]
  }

  /// Reports a failed open of `path`. A refusal is shown in the editor of the workspace until
  /// another file opens or a tab is chosen; any other failure is the dialog.
  public func reportEditorOpenFailure(_ error: Error, workspaceId: String, path: String) {
    guard let refusal = EditorOpenRefusal(error: error, path: path) else {
      // The dialog is about this open now; an earlier refusal described another file.
      editorWorkspace.refusals[workspaceId] = nil
      problem = error.localizedDescription
      return
    }
    editorWorkspace.refusals[workspaceId] = refusal
  }
}
