import Foundation
import SoyehtProtocol

/// The words of the transcript's own controls, shared with the desktop client through
/// `contracts/vectors/agent-cards.json`.
enum AgentTranscriptLabels {
  static let loadEarlier = "Load earlier messages"
}

/// The messages waiting for the agent while it works, as the queue card lists them.
struct AgentQueuePresentation: Equatable, Sendable {
  struct Row: Equatable, Sendable {
    let promptId: String
    /// "From aria" for a message another agent sent.
    let sender: String?
    let text: String
    let attachmentCount: Int?
  }

  static let resumeTitle = "Resume"
  static let removeTitle = "Remove queued message"
  static let sendNowTitle = "Send now"
  static let sendNowHint = "This agent only reads new messages between turns"
  /// "Send now" stays disabled until the engine has a steering method (contract 04, addendum of
  /// 2026-09-11); the desktop client holds the same value (`agent-cards.json`).
  static let sendNowAvailable = false

  let summary: String
  let rows: [Row]

  /// The card is drawn while something waits and while the queue is paused.
  static func isShown(items: [PromptItem], paused: Bool) -> Bool {
    !items.isEmpty || paused
  }

  init(items: [PromptItem], paused: Bool) {
    let fromAgents = items.filter { $0.agentMessage != nil }.count
    let count =
      fromAgents > 0
      ? "\(fromAgents) \(fromAgents == 1 ? "message" : "messages") waiting"
      : "\(items.count) queued"
    summary = paused ? "\(count) · Queue paused after interruption" : count
    rows = items.map { item in
      Row(
        promptId: item.promptId, sender: item.agentMessage.map { "From \($0.displayName)" },
        text: item.text,
        attachmentCount: (item.attachments?.count).flatMap { $0 > 0 ? $0 : nil })
    }
  }
}

/// The words of the sign-in card's controls.
enum AgentSignInLabels {
  static let chooseTerminalHint =
    "Choose a method to open its sign-in terminal. Enter credentials there."
  static let showTerminal = "Show terminal"
  static let cancelSignIn = "Cancel sign-in"
  static let dismiss = "Dismiss"
}

/// The form of an agent's question: one field for each property of the request's schema.
enum AgentElicitationFields {
  struct Field: Equatable, Sendable {
    let key: String
    /// The property's title, or its key when it has none.
    let title: String
  }

  static let cancelTitle = "Cancel"
  static let submitTitle = "Submit"

  /// The fields sorted by key; a schema without properties has none, so nothing can be submitted.
  static func fields(schema: JsonValue) -> [Field] {
    guard case .object(let schema) = schema, case .object(let properties) = schema["properties"]
    else { return [] }
    return properties.keys.sorted().map { key in
      Field(key: key, title: properties[key]?["title"].string ?? key)
    }
  }
}
