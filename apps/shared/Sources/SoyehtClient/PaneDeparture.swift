import Foundation
import SoyehtProtocol

/// How a pane leaves this client's replica. A pane the engine closed, or says it does not hold,
/// simply drops out: nothing is raised to the person, and a late event or answer about it never
/// brings it back or asks the engine about it again.
extension EngineClient {
  /// Whether the engine answered that what a request named does not exist.
  static func isNotFound(_ error: Error) -> Bool {
    guard case ClientFailure.remote(let kind, _, _) = error else { return false }
    return kind == ErrorKind.not_found.rawValue
  }

  /// Handles the failure of a request about `paneId`. When the engine says the pane is gone, it
  /// is dropped without a message and the result is true; any other failure is left to the caller.
  func absorbMissingPane(_ paneId: String, _ error: Error) -> Bool {
    guard Self.isNotFound(error) else { return false }
    removePane(paneId)
    return true
  }

  /// The one way a pane enters the replica: a pane closed since the last snapshot is refused.
  @discardableResult func upsertPane(_ pane: Pane) -> Bool {
    guard !closedPaneIds.contains(pane.paneId) else { return false }
    if let index = panes.firstIndex(where: { $0.paneId == pane.paneId }) {
      panes[index] = pane
    } else {
      panes.append(pane)
    }
    return true
  }

  /// Runs a request about `paneId` without waiting for it. A failure becomes the visible
  /// `problem`, except that the engine not holding the pane drops it quietly.
  func perform(onPane paneId: String, _ action: @escaping @MainActor () async throws -> Void) {
    Task {
      do { try await action() } catch {
        if !absorbMissingPane(paneId, error) { problem = error.localizedDescription }
      }
    }
  }

  /// Tells the engine which pane has focus. False when the pane is gone, which drops it.
  func focusEnginePane(_ paneId: String) async throws -> Bool {
    do {
      _ = try await mutation(
        .paneFocus, PaneFocusParams(paneId: paneId, idempotencyKey: UUID().uuidString))
      return true
    } catch {
      if absorbMissingPane(paneId, error) { return false }
      throw error
    }
  }

  /// Forgets a pane and everything this client kept for it. When it was selected, the selection
  /// moves to another pane of the same workspace before any other workspace's.
  func removePane(_ paneId: String?) {
    guard let paneId else { return }
    closedPaneIds.insert(paneId)
    let workspaceId = panes.first { $0.paneId == paneId }?.workspaceId
    panes.removeAll { $0.paneId == paneId }
    discardTerminalPane(paneId)
    agentPanes.removeValue(forKey: paneId)
    agentWebDrafts.removeValue(forKey: paneId)
    agentTranscriptRevisions.removeValue(forKey: paneId)
    agentTranscriptLoadsInFlight.removeValue(forKey: paneId)
    slashSources.commands.removeValue(forKey: paneId)
    discardEditorPane(paneId)
    discardWritingPane(paneId)
    discardWebPane(paneId)
    guard selectedPaneId == paneId else { return }
    select((panes.first { $0.workspaceId == workspaceId } ?? panes.first)?.paneId)
  }
}
