import Foundation
import Observation

/// The text a person has typed in each agent pane's composer and not sent yet, and the files
/// attached to it.
///
/// The composer view is rebuilt when its place changes: an iPad window resized between a narrow
/// and a wide layout, a rotation that regroups the board, a pane opened from the list and left
/// again. The draft lives here instead of in the view, so none of those loses it. Sending
/// clears it, and so does the pane closing. The staged attachments are kept the same way: a draft
/// with a file on it is as much the person's work as the words.
@MainActor @Observable
public final class AgentPromptDrafts {
  public static let shared = AgentPromptDrafts()

  private var texts: [String: String] = [:]
  /// Not observed: a view asks for the pane's attachments while it is built, and the object
  /// is observable on its own.
  @ObservationIgnored private var staged: [String: AgentComposerAttachments] = [:]

  init() {}

  func text(for paneId: String) -> String { texts[paneId] ?? "" }

  func set(_ text: String, for paneId: String) {
    texts[paneId] = text.isEmpty ? nil : text
  }

  /// The files attached to the pane's next prompt, the same object for as long as the pane lives.
  func attachments(for paneId: String, linkHostFiles: Bool) -> AgentComposerAttachments {
    if let kept = staged[paneId] { return kept }
    let created = AgentComposerAttachments(linkHostFiles: linkHostFiles)
    staged[paneId] = created
    return created
  }

  func discard(_ paneId: String) {
    texts[paneId] = nil
    staged[paneId] = nil
  }
}
