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

  /// Sends what is typed in the pane's composer with the files attached to it, and empties both.
  /// False, with nothing changed, when there is no text to send. `expanding` turns the typed
  /// text into the prompt the engine receives (a slash command becomes its instruction);
  /// `beforeSend` runs once the composer is empty and the prompt is about to leave.
  @discardableResult
  func submit(
    _ paneId: String, to client: EngineClient, expanding: (String) -> String,
    beforeSend: () -> Void = {}
  ) -> Bool {
    let text = text(for: paneId).trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return false }
    set("", for: paneId)
    beforeSend()
    client.sendPrompt(
      paneId, text: expanding(text),
      attachments: attachments(for: paneId, linkHostFiles: !client.pairedDevice).take())
    return true
  }

  func discard(_ paneId: String) {
    texts[paneId] = nil
    staged[paneId] = nil
  }
}
