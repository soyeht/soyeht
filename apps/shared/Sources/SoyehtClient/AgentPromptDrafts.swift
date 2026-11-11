import Foundation
import Observation

/// The text a person has typed in each agent pane's composer and not sent yet, and the files
/// attached to it.
///
/// The composer view is rebuilt when its place changes: an iPad window resized between a narrow
/// and a wide layout, a rotation that regroups the board, a pane opened from the list and left
/// again. The draft lives here instead of in the view, so none of those loses it. Sending
/// clears it once the engine has accepted the prompt, and so does the pane closing. The staged
/// attachments are kept the same way: a draft with a file on it is as much the person's work as
/// the words.
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

  /// Sends what is typed in the pane's composer with the files attached to it. False, with
  /// nothing changed, when there is no text to send. `expanding` turns the typed text into the
  /// prompt the engine receives (a slash command becomes its instruction); `beforeSend` runs once
  /// the composer is empty and the prompt is about to leave.
  ///
  /// The composer empties at once, so the person can write the next thing, but the prompt is only
  /// the engine's once it accepts it. When it does not (a refusal, a timeout, a closed
  /// connection, a prompt over the limit) the text and the files come back exactly as they were
  /// typed, the slash command and not its expansion, and the failure is shown. If the person
  /// has typed since, nothing they wrote is overwritten: the returned text goes first, a blank
  /// line separates it from the newer text, and the returned files go before the newer ones. A
  /// pane closed in the meantime takes its draft with it. A timeout does not say whether the
  /// engine got the prompt; the text comes back and the transcript shows if it did.
  @discardableResult
  func submit(
    _ paneId: String, to client: EngineClient, expanding: (String) -> String,
    beforeSend: () -> Void = {}
  ) -> Bool {
    let typed = text(for: paneId)
    let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return false }
    set("", for: paneId)
    beforeSend()
    let sent = (
      prompt: expanding(text),
      files: attachments(for: paneId, linkHostFiles: !client.pairedDevice).take()
    )
    Task {
      do {
        try await client.deliverPrompt(paneId, text: sent.prompt, attachments: sent.files)
      } catch {
        restore(typed: typed, files: sent.files, to: paneId, client: client)
        client.problem = error.localizedDescription
        return
      }
      do { try await client.loadAgentPane(paneId, replacing: false) } catch {
        client.problem = error.localizedDescription
      }
    }
    return true
  }

  /// Sends the first message of a new agent under the rules of `submit`. The new-pane sheet holds
  /// that message in its form and not in the composer, so it goes through the composer for the
  /// way back: a refused first message is in the new pane's composer, not lost with the sheet.
  func sendFirst(_ text: String, to paneId: String, client: EngineClient) {
    set(text, for: paneId)
    submit(paneId, to: client, expanding: { $0 })
  }

  /// Puts a refused prompt back in its composer, in front of whatever was typed since.
  private func restore(
    typed: String, files: [AgentAttachmentDraft], to paneId: String, client: EngineClient
  ) {
    guard client.panes.contains(where: { $0.paneId == paneId }) else { return }
    let since = text(for: paneId)
    let blank = since.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    set(blank ? typed : typed + "\n\n" + since, for: paneId)
    attachments(for: paneId, linkHostFiles: !client.pairedDevice).restore(files)
  }

  func discard(_ paneId: String) {
    texts[paneId] = nil
    staged[paneId] = nil
  }
}
