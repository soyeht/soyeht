import Foundation
import SoyehtProtocol

/// What an editor pane has to show before its text is on screen. A pane is never left on a
/// spinner: the read is bounded by the request timeout, and a read that fails is a state with
/// a reason, retried by `loadEditorPane`: the person sees content, or an error with a retry.
public enum EditorPaneLoad: Equatable, Sendable {
  case loading
  case ready
  case failed(String)
}

extension EngineClient {
  /// The state of a pane's first page: ready once a page is held, failed once the last read
  /// said why it could not, loading while one is on its way.
  public func editorPaneLoad(_ paneId: String) -> EditorPaneLoad {
    if editorFiles.values.contains(where: { $0.file.paneId == paneId && $0.viewport != nil }) {
      return .ready
    }
    if let reason = loadFailure(.editorPane(paneId: paneId)) { return .failed(reason) }
    return .loading
  }

  /// Reads the page an editor pane shows. A pane whose file this client does not hold (its
  /// creation event named none) finds the file in the engine's tab list first. Nothing here
  /// throws: a failure is the pane's state, and never ends the connection that asked.
  ///
  /// `refreshing` reads the page again when one is held, as when the pane is selected or the
  /// connection is new; a view that only wants a page to exist passes false.
  public func loadEditorPane(_ paneId: String, refreshing: Bool = true) async {
    let load = BackgroundLoad.editorPane(paneId: paneId)
    backgroundLoads[load, default: BackgroundLoadRecord()].failure = nil
    await performBackground(load) { [self] in
      try await readEditorPage(paneId, refreshing: refreshing)
    }.value
  }

  private func readEditorPage(_ paneId: String, refreshing: Bool) async throws {
    guard let pane = panes.first(where: { $0.paneId == paneId && $0.mode == .editor }) else {
      return
    }
    if let view = editorFiles.values.first(where: { $0.file.paneId == paneId }) {
      guard view.viewport == nil || refreshing else { return }
      let held = view.viewport.map { $0.toLine - $0.fromLine }
      _ = try await readEditorFile(
        fileId: view.file.fileId, fromLine: view.viewport?.fromLine ?? 0,
        lineCount: held ?? EditorViewportPaging.pageLines)
      return
    }
    try await loadEditorTabs(workspaceId: pane.workspaceId)
    guard
      let tab = editorWorkspace.tabs[pane.workspaceId]?.first(where: { $0.paneId == paneId })
    else {
      throw ClientFailure.editorCommand("The engine has no file for this editor tab.")
    }
    _ = try await readEditorFile(
      fileId: tab.fileId, fromLine: 0, lineCount: EditorViewportPaging.pageLines)
  }
}
