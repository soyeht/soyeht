import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// Plays the engine for a prompt: it answers `pane.prompt`, can refuse or fail it, and can hold
/// the answer so a test types while the prompt is on its way.
@MainActor private final class FakePromptEngine {
  var prompts: [PanePromptParams] = []
  var failure: ClientFailure?
  var held = false

  func answer(_ method: RequestMethod, _ params: JsonValue) async throws -> JsonValue {
    switch method {
    case .panePrompt:
      prompts.append(try params.decoded(PanePromptParams.self))
      while held { try await Task.sleep(for: .milliseconds(5)) }
      if let failure { throw failure }
      return .object([:])
    default:
      // The transcript that follows an accepted prompt is not what these tests look at.
      throw ClientFailure.remote(ErrorKind.not_found.rawValue, "no transcript here")
    }
  }
}

private let paneId = "refused-pane"

@MainActor private func composer(
  engine: FakePromptEngine
) throws -> (client: EngineClient, drafts: AgentPromptDrafts) {
  let client = EngineClient(socket: "/tmp/agent-prompt-refusal.sock")
  client.connected = true
  client.requestStub = { try await engine.answer($0, $1) }
  let pane = PaneLayoutFixture.pane(paneId, workspace: "work", mode: .agent)
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-11-10T12:00:00Z", pane: pane))
  return (client, AgentPromptDrafts())
}

private func file(_ name: String) -> AgentAttachmentDraft {
  AgentAttachmentDraft(
    kind: .file, name: name, mimeType: "text/plain", byteCount: 5,
    source: .bytes(Data("notes".utf8)))
}

@MainActor private func attachments(_ drafts: AgentPromptDrafts) -> AgentComposerAttachments {
  drafts.attachments(for: paneId, linkHostFiles: false)
}

/// Waits for the engine to have seen `count` prompts and for the client to have settled after the
/// answer.
@MainActor private func settle(_ engine: FakePromptEngine, prompts count: Int = 1) async throws {
  var polls = 0
  while engine.prompts.count < count && polls < 1_000 {
    polls += 1
    try await Task.sleep(for: .milliseconds(5))
  }
  #expect(engine.prompts.count == count)
  try await Task.sleep(for: .milliseconds(150))
}

@MainActor private func send(
  _ drafts: AgentPromptDrafts, _ client: EngineClient, expanding: (String) -> String = { $0 }
) {
  drafts.submit(paneId, to: client, expanding: expanding)
}

@MainActor @Test func aPromptTheEngineRefusesComesBackWithItsFiles() async throws {
  let engine = FakePromptEngine()
  engine.failure = .remote(ErrorKind.pane_busy.rawValue, "The agent is busy.")
  let (client, drafts) = try composer(engine: engine)
  let notes = file("notes.txt")
  drafts.set("  review the diff  ", for: paneId)
  attachments(drafts).add([.success(notes)])

  send(drafts, client)
  try await settle(engine)

  #expect(drafts.text(for: paneId) == "  review the diff  ")
  #expect(attachments(drafts).items == [notes])
  #expect(client.problem == "The agent is busy.")
}

@MainActor @Test func aPromptThatTimedOutComesBackWithItsFiles() async throws {
  let engine = FakePromptEngine()
  engine.failure = .requestTimeout(RequestMethod.panePrompt.rawValue)
  let (client, drafts) = try composer(engine: engine)
  let notes = file("notes.txt")
  drafts.set("run the tests", for: paneId)
  attachments(drafts).add([.success(notes)])

  send(drafts, client)
  // The client sends a mutation again when the connection is what failed, up to three times.
  try await settle(engine, prompts: 3)

  #expect(drafts.text(for: paneId) == "run the tests")
  #expect(attachments(drafts).items == [notes])
}

@MainActor @Test func aPromptSentWhenTheConnectionClosedComesBackWithItsFiles() async throws {
  let engine = FakePromptEngine()
  engine.failure = .disconnected
  let (client, drafts) = try composer(engine: engine)
  let notes = file("notes.txt")
  drafts.set("fix the build", for: paneId)
  attachments(drafts).add([.success(notes)])

  send(drafts, client)
  try await settle(engine, prompts: 3)

  #expect(drafts.text(for: paneId) == "fix the build")
  #expect(attachments(drafts).items == [notes])
}

@MainActor @Test func aSlashCommandComesBackAsTheCommandTheyTyped() async throws {
  let engine = FakePromptEngine()
  engine.failure = .remote(ErrorKind.pane_busy.rawValue, "The agent is busy.")
  let (client, drafts) = try composer(engine: engine)
  drafts.set("/review", for: paneId)

  send(drafts, client) { _ in "Review the staged changes." }
  try await settle(engine)

  #expect(engine.prompts.first?.text == "Review the staged changes.")
  #expect(drafts.text(for: paneId) == "/review")
}

@MainActor @Test func aPromptTooLongForTheEngineComesBackToo() async throws {
  let engine = FakePromptEngine()
  let (client, drafts) = try composer(engine: engine)
  let long = String(repeating: "a", count: ProtocolLimits.promptBytes + 1)
  drafts.set(long, for: paneId)

  send(drafts, client)
  try await Task.sleep(for: .milliseconds(300))

  #expect(engine.prompts.isEmpty)
  #expect(drafts.text(for: paneId) == long)
}

@MainActor @Test func whatIsTypedWhileThePromptIsOnItsWayIsNeverOverwritten() async throws {
  let engine = FakePromptEngine()
  engine.held = true
  engine.failure = .remote(ErrorKind.pane_busy.rawValue, "The agent is busy.")
  let (client, drafts) = try composer(engine: engine)
  let first = file("first.txt")
  let second = file("second.txt")
  drafts.set("the refused prompt", for: paneId)
  attachments(drafts).add([.success(first)])

  send(drafts, client)
  var polls = 0
  while engine.prompts.isEmpty && polls < 1_000 {
    polls += 1
    try await Task.sleep(for: .milliseconds(5))
  }
  // The composer is free while the engine thinks about it.
  #expect(drafts.text(for: paneId).isEmpty)
  drafts.set("the next thought", for: paneId)
  attachments(drafts).add([.success(second)])
  engine.held = false
  try await settle(engine)

  // The refused prompt comes back in front of what was typed since, and nothing is lost.
  #expect(drafts.text(for: paneId) == "the refused prompt\n\nthe next thought")
  #expect(attachments(drafts).items == [first, second])
}

@MainActor @Test func anAcceptedPromptLeavesTheComposerEmpty() async throws {
  let engine = FakePromptEngine()
  let (client, drafts) = try composer(engine: engine)
  drafts.set("ship it", for: paneId)
  attachments(drafts).add([.success(file("notes.txt"))])

  send(drafts, client)
  try await settle(engine)

  #expect(engine.prompts.first?.text == "ship it")
  #expect(engine.prompts.first?.attachments?.count == 1)
  #expect(drafts.text(for: paneId).isEmpty)
  #expect(attachments(drafts).items.isEmpty)
}

@MainActor @Test func aPaneClosedDuringTheSendIsNotBroughtBack() async throws {
  let engine = FakePromptEngine()
  engine.held = true
  engine.failure = .remote(ErrorKind.not_found.rawValue, "No such pane.")
  let (client, drafts) = try composer(engine: engine)
  drafts.set("for a pane that is gone", for: paneId)

  send(drafts, client)
  var polls = 0
  while engine.prompts.isEmpty && polls < 1_000 {
    polls += 1
    try await Task.sleep(for: .milliseconds(5))
  }
  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string(paneId)]))
  engine.held = false
  try await settle(engine)

  #expect(drafts.text(for: paneId).isEmpty)
  #expect(attachments(drafts).items.isEmpty)
}
