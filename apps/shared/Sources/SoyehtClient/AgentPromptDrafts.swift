import Foundation
import Observation

/// The text a person has typed in each agent pane's composer and not sent yet.
///
/// The composer view is rebuilt when its place changes: an iPad window resized between a narrow
/// and a wide layout, a rotation that regroups the board, a pane opened from the list and left
/// again. The draft lives here instead of in the view, so none of those loses it. Sending
/// clears it, and so does the pane closing.
@MainActor @Observable
public final class AgentPromptDrafts {
  public static let shared = AgentPromptDrafts()

  private var texts: [String: String] = [:]

  init() {}

  func text(for paneId: String) -> String { texts[paneId] ?? "" }

  func set(_ text: String, for paneId: String) {
    texts[paneId] = text.isEmpty ? nil : text
  }

  func discard(_ paneId: String) { texts[paneId] = nil }
}
