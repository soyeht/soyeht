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
  var tabsFailure: ClientFailure?
  var readFailure: ClientFailure?
  /// The files read, in order.
  var readFiles: [String] = []

  func answer(_ method: RequestMethod, _ params: JsonValue) throws -> JsonValue {
    requests.append(method)
    if let failure { throw failure }
    switch method {
    case .editorTabsList:
      if let tabsFailure { throw tabsFailure }
      return try .encoded(EditorTabsListResult(workspaceId: "workspace-1", tabs: tabs))
    case .fileRead:
      let fileId = params["fileId"].string ?? "file-1"
      readFiles.append(fileId)
      if let readFailure { throw readFailure }
      let first = params["fromLine"].int ?? 0
      return try .encoded(readResult(fileId: fileId, from: UInt32(first)))
    default:
      throw ClientFailure.invalidFrame
    }
  }
}

private func document(fileId: String = "file-1") -> FileDocument {
  FileDocument(
    fileId: fileId, paneId: "pane-1", workspaceId: "workspace-1", path: "README.md",
    language: .markdown, revision: "1", diskRevision: "1", lineCount: 2, byteCount: "9",
    dirty: false, conflicted: false, watching: true)
}

private func readResult(fileId: String, from: UInt32) -> FileReadResult {
  FileReadResult(
    file: document(fileId: fileId), source: .buffer, fromLine: from, toLine: from + 2,
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

/// Plays one step of a case of `contracts/vectors/editor.json` against the client: what the
/// engine answers, then the person or the client asking.
@MainActor private func play(
  _ step: JsonValue, on client: EngineClient, with engine: FakeEditorEngine
) async {
  let answers = step["engine"]
  engine.tabs =
    answers["tabs"].string == "omits"
    ? []
    : [
      EditorTab(
        paneId: "pane-1", fileId: answers["file"].string ?? "file-1", path: "README.md",
        preview: false)
    ]
  engine.tabsFailure =
    answers["tabs"].string == "fails"
    ? .requestTimeout(RequestMethod.editorTabsList.rawValue) : nil
  engine.readFailure =
    answers["read"].string == "fails" ? .requestTimeout(RequestMethod.fileRead.rawValue) : nil
  engine.requests = []
  engine.readFiles = []

  await client.loadEditorPane("pane-1", refreshing: step["do"].string == "refresh")
}

@MainActor private func requests(of engine: FakeEditorEngine) -> [String] {
  var reads = engine.readFiles
  return engine.requests.compactMap { method in
    switch method {
    case .editorTabsList: method.rawValue
    case .fileRead: "\(method.rawValue):\(reads.isEmpty ? "" : reads.removeFirst())"
    default: nil
    }
  }
}

@MainActor @Test func theLoadStatesAndTheirTransitionsFollowTheEditorVector() async throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector["paneLoad"], "cases") {
    let engine = FakeEditorEngine()
    let client = try client(with: engine, held: item["fileHeld"].bool == true)
    let label = name(item)
    #expect(state(of: client.editorPaneLoad("pane-1")) == item["initial"].string, "\(label)")
    for step in item["steps"].array ?? [] {
      await play(step, on: client, with: engine)
      let load = client.editorPaneLoad("pane-1")
      #expect(state(of: load) == step["state"].string, "\(label)")
      if let expected = step["requests"].array {
        #expect(requests(of: engine) == expected.compactMap(\.string), "\(label)")
      }
      guard case .failed(let reason) = load else { continue }
      switch step["reason"].string {
      case "noFile": #expect(reason == ClientPolicy.editorNoFile, "\(label)")
      default: #expect(!reason.isEmpty, "\(label)")
      }
    }
  }
}

private func state(of load: EditorPaneLoad) -> String {
  switch load {
  case .loading: "loading"
  case .ready: "ready"
  case .failed: "failed"
  }
}

private func name(_ item: JsonValue) -> String { item["name"].string ?? "" }
