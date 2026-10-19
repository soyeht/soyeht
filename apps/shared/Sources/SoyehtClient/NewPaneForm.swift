import Foundation
import SoyehtProtocol

/// What a new pane runs: a shell, an agent, or a new writing document.
public enum NewPaneKind: Equatable, Sendable {
  case terminal
  case agent
  case writing(WritingSection)

  public init(mode: PaneMode) {
    self = mode == .agent ? .agent : .terminal
  }
}

/// Why the "create" button of the new-pane form is off. One rule for every client, so the
/// Mac sheet and the iPhone and iPad sheet agree on what they accept.
public enum NewPaneBlocker: Equatable, Sendable {
  case noFolder
  case noAgent
  case agentUnavailable
  case promptTooLong
  case noTitle

  public var explanation: String {
    switch self {
    case .noFolder: "Choose a folder on the Mac."
    case .noAgent: "Choose an agent."
    case .agentUnavailable: "This agent is not ready. Choose another or check it again."
    case .promptTooLong: "The first message is too long."
    case .noTitle: "Give the document a title."
    }
  }
}

/// The choices behind the new-pane sheet: where it runs, what it runs, and for an agent which
/// harness, model, effort and permission mode. The sheet edits it; the shared client creates
/// the pane from it.
public struct NewPaneForm: Equatable, Sendable {
  public var kind: NewPaneKind
  /// Absolute path of the folder on the Mac. The pane's workspace is the one at this path.
  public var directory = ""
  public var harness = ""
  /// Empty means the harness default.
  public var model = ""
  /// Empty means the harness default.
  public var effort = ""
  /// `nil` lets the engine apply the harness's preferred mode.
  public var permissionMode: String?
  /// The agent's first message; blank sends nothing.
  public var prompt = ""
  /// The new document's title, for a writing pane.
  public var title = ""

  public init(kind: NewPaneKind = .agent, directory: String = "") {
    self.kind = kind
    self.directory = directory
  }

  /// Choosing another harness drops the model, effort and mode of the previous one.
  public mutating func chooseHarness(_ id: String) {
    guard id != harness else { return }
    harness = id
    model = ""
    effort = ""
    permissionMode = nil
  }

  /// Keeps the chosen harness while it is available; otherwise takes the preferred one or the
  /// most recently ready one. Does nothing while no harness can run.
  public mutating func keepOrChooseHarness(in harnesses: [HarnessInfo], preferredId: String?) {
    if harnesses.first(where: { $0.id == harness })?.available == true { return }
    guard
      let first = HarnessPickerModel.defaultHarness(in: harnesses, preferredId: preferredId)
    else { return }
    chooseHarness(first.id)
  }

  public var firstPrompt: String { prompt.trimmingCharacters(in: .whitespacesAndNewlines) }

  /// What stops the pane from being created, or `nil` when it can be.
  public func blocker(harnesses: [HarnessInfo], submitting: Bool = false) -> NewPaneBlocker? {
    if directory.isEmpty { return .noFolder }
    switch kind {
    case .terminal:
      return nil
    case .agent:
      guard !harness.isEmpty else { return .noAgent }
      guard harnesses.first(where: { $0.id == harness })?.available == true else {
        return .agentUnavailable
      }
      return firstPrompt.utf8.count > ProtocolLimits.promptBytes ? .promptTooLong : nil
    case .writing:
      return WritingDocumentDraft(title: title).submission(creating: submitting) == nil
        ? .noTitle : nil
    }
  }

  public func canCreate(harnesses: [HarnessInfo], submitting: Bool = false) -> Bool {
    !submitting && blocker(harnesses: harnesses) == nil
  }

  /// The agent chip's text: the chosen model, else the harness, else a prompt to choose.
  public func agentLabel(harnesses: [HarnessInfo], catalogue: AgentCatalogue?) -> String {
    catalogue?.models.first { $0.id == model }?.name
      ?? harnesses.first { $0.id == harness }?.name ?? "Choose agent"
  }
}

extension EngineClient {
  /// Creates the pane the form describes in its folder's workspace, opening that workspace
  /// when the Mac has none for the folder yet. Returns the new pane's identifier. The agent's
  /// first message is not sent here: the caller sends `form.firstPrompt` once the pane shows.
  public func createPane(_ form: NewPaneForm) async throws -> String {
    let directory = URL(fileURLWithPath: form.directory, isDirectory: true)
    switch form.kind {
    case .terminal:
      return try await createTerminalPane(directory: directory)
    case .agent:
      return try await createAgentPaneNow(
        directory: directory, harness: form.harness,
        model: form.model.isEmpty ? nil : form.model,
        effort: form.effort.isEmpty ? nil : form.effort,
        permissionMode: form.permissionMode)
    case .writing(let section):
      guard
        let submission = WritingDocumentDraft(title: form.title).submission(creating: false)
      else { throw ClientFailure.invalidFrame }
      let workspace = try await workspaceId(
        for: directory, path: directory.standardizedFileURL.path)
      return try await createWritingDocument(
        workspaceId: workspace, section: section, title: submission.title
      ).pane.paneId
    }
  }
}
