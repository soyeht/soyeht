import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// Plays the engine for the requests an editor pane makes to get its first page.
@MainActor private final class FakeEditorEngine {
  var requests: [RequestMethod] = []
  var tabs: [EditorTab] = [
    EditorTab(paneId: "pane-1", fileId: "file-1", path: "README.md", preview: false)
  ]
  var failure: ClientFailure?

  func answer(_ method: RequestMethod, _ params: JsonValue) throws -> JsonValue {
    requests.append(method)
    if let failure { throw failure }
    switch method {
    case .editorTabsList:
      return try .encoded(EditorTabsListResult(workspaceId: "workspace-1", tabs: tabs))
    case .fileRead:
      let first = params["fromLine"].int ?? 0
      return try .encoded(readResult(from: UInt32(first)))
    default:
      throw ClientFailure.invalidFrame
    }
  }
}

private func document() -> FileDocument {
  FileDocument(
    fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1", path: "README.md",
    language: .markdown, revision: "1", diskRevision: "1", lineCount: 2, byteCount: "9",
    dirty: false, conflicted: false, watching: true)
}

private func readResult(from: UInt32) -> FileReadResult {
  FileReadResult(
    file: document(), source: .buffer, fromLine: from, toLine: from + 2,
    lines: [
      EditorLine(line: from, text: "# Soyeht", highlights: []),
      EditorLine(line: from + 1, text: "", highlights: []),
    ])
}

/// A client that holds the pane the engine created and, when `held`, its file without a page.
@MainActor private func client(
  with engine: FakeEditorEngine, held: Bool
) throws -> EngineClient {
  let client = EngineClient(socket: "/tmp/editor-pane-load.sock")
  client.requestStub = { try engine.answer($0, $1) }
  let pane = Pane(
    paneId: "pane-1", workspaceId: "workspace-1", name: "README.md", cwd: "/repo",
    mode: .editor, state: .idle, source: .protocol, confidence: 1, phrase: "editing README.md",
    generation: 1, cols: 80, rows: 24, record: false)
  try client.applyPaneCreated(
    PaneCreatedEvent(
      eventId: "1", at: "2026-10-17T12:00:00Z", pane: pane, file: held ? document() : nil))
  return client
}

@MainActor @Test func aPaneWhoseFileThisClientDoesNotHoldFindsItInTheEnginesTabs() async throws {
  let engine = FakeEditorEngine()
  let client = try client(with: engine, held: false)
  #expect(client.editorPaneLoad("pane-1") == .loading)

  await client.loadEditorPane("pane-1")

  #expect(engine.requests == [.editorTabsList, .fileRead])
  #expect(client.editorPaneLoad("pane-1") == .ready)
  #expect(client.editorFiles["file-1"]?.viewport?.lines.first?.text == "# Soyeht")
}

@MainActor @Test func aPaneTheEngineListsNoTabForIsAFailureNotASpinner() async throws {
  let engine = FakeEditorEngine()
  engine.tabs = []
  let client = try client(with: engine, held: false)

  await client.loadEditorPane("pane-1")

  guard case .failed(let reason) = client.editorPaneLoad("pane-1") else {
    Issue.record("expected a failure, got \(client.editorPaneLoad("pane-1"))")
    return
  }
  #expect(reason.contains("no file"))
  #expect(client.loadFailure(.editorPane(paneId: "pane-1")) == reason)
}

@MainActor @Test func aFailedFirstReadIsAnErrorThatARetryClears() async throws {
  let engine = FakeEditorEngine()
  let client = try client(with: engine, held: true)
  engine.failure = .requestTimeout(RequestMethod.fileRead.rawValue)
  #expect(client.editorPaneLoad("pane-1") == .loading)

  await client.loadEditorPane("pane-1", refreshing: false)
  guard case .failed = client.editorPaneLoad("pane-1") else {
    Issue.record("expected a failure, got \(client.editorPaneLoad("pane-1"))")
    return
  }
  #expect(client.editorFiles["file-1"]?.viewport == nil)

  engine.failure = nil
  await client.loadEditorPane("pane-1", refreshing: false)
  #expect(client.editorPaneLoad("pane-1") == .ready)
  #expect(client.loadFailure(.editorPane(paneId: "pane-1")) == nil)
}

@MainActor @Test func aFailedRefreshKeepsTheContentTheViewAlreadyHas() async throws {
  let engine = FakeEditorEngine()
  let client = try client(with: engine, held: true)
  await client.loadEditorPane("pane-1")
  #expect(client.editorPaneLoad("pane-1") == .ready)

  engine.failure = .disconnected
  await client.loadEditorPane("pane-1")

  #expect(client.editorPaneLoad("pane-1") == .ready)
  #expect(client.editorFiles["file-1"]?.viewport != nil)
}

@MainActor @Test func aViewThatOnlyWantsAPageDoesNotReadOneItAlreadyHolds() async throws {
  let engine = FakeEditorEngine()
  let client = try client(with: engine, held: true)
  await client.loadEditorPane("pane-1", refreshing: false)
  engine.requests = []

  await client.loadEditorPane("pane-1", refreshing: false)

  #expect(engine.requests.isEmpty)
}

@MainActor @Test func closingAPaneForgetsItsLoadFailure() async throws {
  let engine = FakeEditorEngine()
  let client = try client(with: engine, held: true)
  engine.failure = .disconnected
  await client.loadEditorPane("pane-1")
  #expect(client.loadFailure(.editorPane(paneId: "pane-1")) != nil)

  client.discardEditorPane("pane-1")

  #expect(client.loadFailure(.editorPane(paneId: "pane-1")) == nil)
}
