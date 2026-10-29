import Foundation
import SoyehtProtocol

/// The question a person is asked before something closes, with the word on its button.
public struct CloseConfirmation: Equatable, Sendable {
  public let title: String
  public let message: String
  public let confirmTitle: String

  public init(title: String, message: String, confirmTitle: String = "Close") {
    self.title = title
    self.message = message
    self.confirmTitle = confirmTitle
  }

  /// The question for a pane that runs a process: a terminal, an agent or a web page.
  public static let runningPane = CloseConfirmation(
    title: "Close pane?", message: "The process in this pane will be terminated.")
}

/// When closing a pane or a workspace asks first. The Mac and the paired iPhone and iPad share
/// it, so both answer the same way.
public enum PaneClosePolicy {
  /// The question to ask before closing `pane`, or `nil` when it closes at once. A pane that
  /// runs a process always asks. A document asks only while it holds unsaved changes, and an
  /// editor tab only when its draft conflicts with the disk. `file` is the pane's open file.
  public static func confirmation(for pane: Pane, file: FileDocument?) -> CloseConfirmation? {
    switch pane.mode {
    case .terminal, .agent, .web:
      return .runningPane
    case .writing:
      let name = fileName(file, fallback: pane.name)
      switch WritingChromePolicy().closeDisposition(fileName: name, dirty: file?.dirty == true) {
      case .closeImmediately: return nil
      case .confirm(let title, let message):
        return CloseConfirmation(title: title, message: message)
      }
    case .editor:
      guard EditorTabPolicy.closeNeedsConfirmation(file) else { return nil }
      return CloseConfirmation(
        title: "Close \(fileName(file, fallback: pane.name))?",
        message: "Its changes conflict with the file on disk. The draft stays on the Mac.")
    }
  }

  /// The question to ask before closing a workspace, or `nil` when it holds no pane. Closing a
  /// workspace closes every pane in it, so it asks whenever there is one, whether or not
  /// anything runs there; its folder and files are not touched. This is the question for a
  /// person's own close (the tab's button); the engine's `workspace.close` itself never asks,
  /// so a close a script or an agent sends does not wait on a dialog.
  public static func confirmation(for workspace: Workspace, paneCount: Int) -> CloseConfirmation? {
    guard paneCount > 0 else { return nil }
    let panes = paneCount == 1 ? "its pane" : "its \(paneCount) panes"
    return CloseConfirmation(
      title: "Close \(workspace.name)?",
      message: "The processes in \(panes) will be terminated. The folder stays on the Mac.",
      confirmTitle: "Close workspace")
  }

  private static func fileName(_ file: FileDocument?, fallback: String) -> String {
    file.map { URL(fileURLWithPath: $0.path).lastPathComponent } ?? fallback
  }
}

extension EngineClient {
  /// The question to ask before closing the pane, or `nil` when it closes at once.
  public func closeConfirmation(forPane paneId: String) -> CloseConfirmation? {
    guard let pane = panes.first(where: { $0.paneId == paneId }) else { return nil }
    return PaneClosePolicy.confirmation(
      for: pane, file: editorFiles.values.first { $0.file.paneId == paneId }?.file)
  }

  /// The question to ask before closing the workspace, or `nil` when it closes at once.
  public func closeConfirmation(forWorkspace workspaceId: String) -> CloseConfirmation? {
    guard let workspace = workspaces.first(where: { $0.workspaceId == workspaceId }) else {
      return nil
    }
    return PaneClosePolicy.confirmation(
      for: workspace, paneCount: panes.filter { $0.workspaceId == workspaceId }.count)
  }

  /// Closes a workspace and its panes, after what was typed in its documents has reached the
  /// engine, as `closePaneSavingEdits` does for one pane. When the engine refuses, the
  /// workspace stays and the person is told so. Returns whether it closed.
  @discardableResult
  public func closeWorkspaceReportingFailure(_ workspaceId: String) async -> Bool {
    for editor in editorFiles.values where editor.file.workspaceId == workspaceId {
      await flushEditorEdits(fileId: editor.file.fileId)
    }
    do {
      try await closeWorkspace(workspaceId)
      return true
    } catch {
      problem = "Couldn't close the workspace. It is still available."
      return false
    }
  }

  /// Closes a pane the way its kind needs: an editor tab hands its place to the next tab, and a
  /// document saves what the person typed first. When the engine refuses an edit the pane closes
  /// all the same: the replica has dropped the unsent edits, reported `problem` and read the
  /// file again, so a pane kept open would show text the engine never took (the Mac's text view
  /// can still hold the typed characters, but they no longer match the file), and a second
  /// close would lose them just as well.
  public func closePaneSavingEdits(_ paneId: String) async {
    guard let pane = panes.first(where: { $0.paneId == paneId }) else { return }
    switch pane.mode {
    case .editor:
      do { try await closeEditorTab(paneId) } catch { problem = error.localizedDescription }
    case .writing:
      if let fileId = editorFiles.values.first(where: { $0.file.paneId == paneId })?.file.fileId {
        await flushEditorEdits(fileId: fileId)
      }
      closePane(paneId)
    case .terminal, .agent, .web:
      closePane(paneId)
    }
  }
}
