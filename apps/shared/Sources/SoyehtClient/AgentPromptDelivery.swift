import Foundation
import SoyehtProtocol

extension EngineClient {
  public func sendPrompt(
    _ paneId: String, text: String, attachments: [AgentAttachmentDraft] = []
  ) {
    perform {
      try await self.sendPromptNow(paneId, text: text, attachments: attachments)
    }
  }

  public func sendPromptNow(
    _ paneId: String, text: String, attachments drafts: [AgentAttachmentDraft] = []
  ) async throws {
    try await deliverPrompt(paneId, text: text, attachments: drafts)
    try await loadAgentPane(paneId, replacing: false)
  }

  /// Sends the prompt and returns once the engine has accepted it. Throws when it did not: a
  /// refusal, a timeout, a connection that closed, a prompt over the limit. What follows the
  /// acceptance (reading the transcript again) is not part of it, so a caller that keeps the
  /// prompt until this returns never keeps one the engine has.
  func deliverPrompt(
    _ paneId: String, text: String, attachments drafts: [AgentAttachmentDraft]
  ) async throws {
    let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    let draft = agentWebDrafts[paneId]
    let prompt = draft.map { $0.markdown + "\n\n## Request\n\n" + text } ?? text
    guard prompt.utf8.count <= ProtocolLimits.promptBytes else {
      throw ClientFailure.invalidFrame
    }
    let (attachments, frames) = Self.promptAttachments(web: draft, drafts: drafts)
    _ = try await mutation(
      .panePrompt,
      PanePromptParams(
        paneId: paneId, text: prompt, origin: .human,
        attachments: attachments.isEmpty ? nil : attachments,
        idempotencyKey: UUID().uuidString),
      attachments: frames)
    agentWebDrafts.removeValue(forKey: paneId)
  }
}
