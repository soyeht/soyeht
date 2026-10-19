import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// Plays an engine whose `file.diff` answers only when the test lets it, as git does on a cold
/// repository, and counts the diffs asked for.
@MainActor private final class SlowDiffEngine {
  var diffRequests: [String] = []
  var diffFailure: ClientFailure?
  private var waiting: [CheckedContinuation<Void, Never>] = []

  func answer(_ method: RequestMethod, _ params: JsonValue) async throws -> JsonValue {
    switch method {
    case .fileDiff:
      let revision = params["revision"].string ?? "?"
      diffRequests.append(revision)
      await withCheckedContinuation { waiting.append($0) }
      if let diffFailure { throw diffFailure }
      return try .encoded(
        FileDiffResult(fileId: "file-1", revision: revision, hunks: [], blame: []))
    default:
      throw ClientFailure.invalidFrame
    }
  }

  func release() {
    let held = waiting
    waiting = []
    for continuation in held { continuation.resume() }
  }

  func waitForRequest(_ count: Int) async {
    while diffRequests.count < count { await Task.yield() }
  }

  /// Asks for the diff and lets the engine answer it, as the `count`th request.
  func answerDiff(of client: EngineClient, request count: Int) async {
    let load = Task { await client.loadEditorDiff(fileId: "file-1") }
    await waitForRequest(count)
    release()
    await load.value
  }
}

private func document(revision: String = "1") -> FileDocument {
  FileDocument(
    fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1", path: "notes.txt",
    language: .plain_text, revision: revision, diskRevision: "1", lineCount: 2, byteCount: "9",
    dirty: false, conflicted: false, watching: true)
}

private func page(revision: String = "1") -> FileReadResult {
  FileReadResult(
    file: document(revision: revision), source: .disk, fromLine: 0, toLine: 2,
    lines: [
      EditorLine(line: 0, text: "hello", highlights: []),
      EditorLine(line: 1, text: "", highlights: []),
    ])
}

/// A client that holds an opened file with its first page, as `file.open` leaves it.
@MainActor private func client(with engine: SlowDiffEngine, page held: Bool = true)
  -> EngineClient
{
  let client = EngineClient(socket: "/tmp/editor-diff-load.sock")
  client.requestStub = { try await engine.answer($0, $1) }
  client.acceptEditorRead(page())
  if !held { client.editorFiles["file-1"]?.viewport = nil }
  return client
}

@MainActor @Test func theTextIsOnScreenWhileTheDiffIsStillBeingRead() async throws {
  let engine = SlowDiffEngine()
  let client = client(with: engine)

  let load = Task { await client.loadEditorDiff(fileId: "file-1") }
  await engine.waitForRequest(1)

  // The page is held and readable, and no diff has arrived: nothing waits for git.
  #expect(client.editorFiles["file-1"]?.viewport?.lines.first?.text == "hello")
  #expect(client.editorFiles["file-1"]?.readyDiff == nil)
  engine.release()
  await load.value
  #expect(client.editorFiles["file-1"]?.readyDiff?.revision == "1")
}

@MainActor @Test func aRevisionIsAskedForOnceWhateverNumberOfPanesAsk() async throws {
  let engine = SlowDiffEngine()
  let client = client(with: engine)

  let first = Task { await client.loadEditorDiff(fileId: "file-1") }
  await engine.waitForRequest(1)
  // The pane appears again while the diff is on its way, and again once it has arrived.
  await client.loadEditorDiff(fileId: "file-1")
  engine.release()
  await first.value
  await client.loadEditorDiff(fileId: "file-1")

  #expect(engine.diffRequests == ["1"])
}

@MainActor @Test func nothingIsAskedBeforeTheFirstPageIsHeld() async throws {
  let engine = SlowDiffEngine()
  let client = client(with: engine, page: false)

  await client.loadEditorDiff(fileId: "file-1")

  #expect(engine.diffRequests.isEmpty)
}

@MainActor @Test func aNewRevisionIsAskedForAndTheOldDiffIsNotShownForIt() async throws {
  let engine = SlowDiffEngine()
  let client = client(with: engine)
  await engine.answerDiff(of: client, request: 1)
  #expect(client.editorFiles["file-1"]?.readyDiff != nil)

  client.acceptEditorRead(page(revision: "2"))
  #expect(client.editorFiles["file-1"]?.readyDiff == nil)
  await engine.answerDiff(of: client, request: 2)

  #expect(engine.diffRequests == ["1", "2"])
  #expect(client.editorFiles["file-1"]?.readyDiff?.revision == "2")
}

@MainActor @Test func aCallerThatMovesOnBeforeTheSettleTimeAsksForNothing() async throws {
  let engine = SlowDiffEngine()
  let client = client(with: engine)

  let passing = Task { await client.loadEditorDiff(fileId: "file-1") }
  passing.cancel()
  await passing.value

  #expect(engine.diffRequests.isEmpty)
  #expect(client.editorFiles["file-1"]?.diff.revision == nil)
}

@MainActor @Test func aFailedDiffIsAProblemOfThatRevisionAndIsNotRetriedForIt() async throws {
  let engine = SlowDiffEngine()
  engine.diffFailure = .remote(ErrorKind.git_unavailable.rawValue, "Git is not available")
  let client = client(with: engine)

  await engine.answerDiff(of: client, request: 1)
  await client.loadEditorDiff(fileId: "file-1")

  #expect(client.editorFiles["file-1"]?.readyDiff == nil)
  #expect(client.editorFiles["file-1"]?.diffProblem != nil)
  #expect(engine.diffRequests == ["1"])
}
