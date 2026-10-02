import Foundation
import Observation
import SoyehtProtocol

/// The three buffer-search toggles: case, whole word and regular expression.
public struct EditorFindOptions: Equatable, Sendable {
  public var caseSensitive: Bool
  public var wholeWord: Bool
  public var regex: Bool

  public init(caseSensitive: Bool = false, wholeWord: Bool = false, regex: Bool = false) {
    self.caseSensitive = caseSensitive
    self.wholeWord = wholeWord
    self.regex = regex
  }

  public func query(_ text: String) -> FileFindQuery {
    FileFindQuery(text: text, caseSensitive: caseSensitive, wholeWord: wholeWord, regex: regex)
  }
}

extension EngineClient {
  /// Finds the next or previous match over the whole file in the engine,
  /// loads the page that holds it and selects it there. Matches on the
  /// landing page come back in `visible` for highlighting.
  @discardableResult
  public func findInEditorFile(
    _ fileId: String, query: FileFindQuery, from: TextPosition, direction: FileFindDirection
  ) async throws -> FileFindResult {
    await settleEditorWrites(fileId)
    guard let view = editorFiles[fileId], let loaded = view.viewport else {
      throw ClientFailure.invalidSnapshot
    }
    var result = try await requestEditorFind(
      fileId, revision: view.file.revision, query: query, from: from, direction: direction,
      page: loaded)
    guard let current = result.current else { return result }
    let landing = EditorViewportPaging.landingStart(
      line: current.range.start.line, loadedFrom: loaded.fromLine, loadedTo: loaded.toLine)
    if landing != loaded.fromLine {
      let page = try await readEditorFile(
        fileId: fileId, revision: result.revision, fromLine: landing,
        lineCount: EditorViewportPaging.pageLines)
      result = try await requestEditorFind(
        fileId, revision: result.revision, query: query, from: current.range.start,
        direction: .next, page: page)
    }
    editorNavigationTarget = EditorNavigationTarget(
      id: UUID().uuidString, fileId: fileId, position: current.range.start,
      selectionEnd: current.range.end)
    return result
  }

  /// Replaces the match at `range`, or every match when `range` is nil, as one
  /// engine edit and one undo step, then reloads the page that shows it.
  @discardableResult
  public func replaceInEditorFile(
    _ fileId: String, query: FileFindQuery, replacement: String, range: SoyehtProtocol.TextRange?
  ) async throws -> FileReplaceResult {
    await settleEditorWrites(fileId)
    invalidateEditorRefresh(fileId)
    guard let revision = editorFiles[fileId]?.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let result = try await mutation(
      "file.replace",
      FileReplaceParams(
        fileId: fileId, baseRevision: revision, query: query, replacement: replacement,
        range: range, idempotencyKey: UUID().uuidString)
    ).decoded(FileReplaceResult.self)
    upsertEditor(result.file) { $0.changedLines = result.changedLines }
    if result.replaced > 0 { try await reloadEditorPage(fileId, showing: result.cursor?.line) }
    return result
  }

  private func requestEditorFind(
    _ fileId: String, revision: String, query: FileFindQuery, from: TextPosition,
    direction: FileFindDirection, page: FileReadResult
  ) async throws -> FileFindResult {
    try await request(
      "file.find",
      .encoded(
        FileFindParams(
          fileId: fileId, revision: revision, query: query, from: from, direction: direction,
          visibleFromLine: page.fromLine, visibleLineCount: page.toLine - page.fromLine))
    ).decoded(FileFindResult.self)
  }
}

/// Find and replace for one open file, shared by every editor surface. The
/// view binds the fields and forwards commands with the current selection in
/// document coordinates; the engine does the searching.
@MainActor @Observable
public final class EditorFindController {
  public let fileId: String
  public private(set) var isPresented = false
  public var showsReplace = false
  public private(set) var query = ""
  public var replacement = ""
  public private(set) var options = EditorFindOptions()
  public private(set) var result: FileFindResult?
  public private(set) var problem: String?
  /// Bumped when the query field should take focus.
  public private(set) var fieldFocusRequest = 0
  /// Bumped when focus should return to the text.
  public private(set) var editorFocusRequest = 0
  @ObservationIgnored private let client: EngineClient
  @ObservationIgnored private var anchor = TextPosition(line: 0, column: 0)
  @ObservationIgnored private var generation = 0
  @ObservationIgnored private var pending: Task<Void, Never>?
  @ObservationIgnored private var replacing = false

  public init(client: EngineClient, fileId: String) {
    self.client = client
    self.fileId = fileId
  }

  /// "3 of 41", "No results", the engine's complaint, or nothing yet.
  public var status: String { Self.status(query: query, result: result, problem: problem) }

  static func status(query: String, result: FileFindResult?, problem: String?) -> String {
    if let problem { return problem }
    guard !query.isEmpty, let result else { return "" }
    guard let current = result.current, result.total > 0 else { return "No results" }
    return "\(UInt64(current.index) + 1) of \(result.total)"
  }

  public var hasMatches: Bool { (result?.total ?? 0) > 0 }

  /// Opens the bar (Cmd+F, or Cmd+Option+F with `replace`), seeding the
  /// query from a one-line selection and searching from the selection start.
  public func present(replace: Bool, selection: SoyehtProtocol.TextRange?, selectedText: String?) {
    isPresented = true
    showsReplace = replace
    fieldFocusRequest += 1
    if let selection { anchor = selection.start }
    if let selectedText, !selectedText.isEmpty, !selectedText.contains("\n") {
      query = selectedText
    }
    search(from: anchor, direction: .next, debounce: false)
  }

  public func close() {
    pending?.cancel()
    generation += 1
    isPresented = false
    result = nil
    problem = nil
    editorFocusRequest += 1
  }

  public func setQuery(_ text: String) {
    guard text != query else { return }
    query = text
    search(from: anchor, direction: .next, debounce: true)
  }

  public func setOptions(_ value: EditorFindOptions) {
    guard value != options else { return }
    options = value
    search(from: anchor, direction: .next, debounce: false)
  }

  /// Cmd+G: the first match after the selection, wrapping at the end.
  public func next(after selection: SoyehtProtocol.TextRange?) {
    search(from: selection?.end ?? anchor, direction: .next, debounce: false)
  }

  /// Cmd+Shift+G: the last match before the selection, wrapping at the start.
  public func previous(before selection: SoyehtProtocol.TextRange?) {
    search(from: selection?.start ?? anchor, direction: .previous, debounce: false)
  }

  /// Next and previous from the bar itself step from the current match.
  public func nextMatch() { next(after: result?.current?.range) }

  public func previousMatch() { previous(before: result?.current?.range) }

  /// Re-counts after the text changed, staying on the current match. A
  /// replacement in flight already searches again when it lands.
  public func refresh() {
    guard isPresented, !replacing else { return }
    search(from: result?.current?.range.start ?? anchor, direction: .next, debounce: true)
  }

  /// Replaces the current match and moves on to the next one.
  public func replaceCurrent() {
    guard let current = result?.current else { return }
    perform(range: current.range)
  }

  public func replaceAll() {
    guard hasMatches else { return }
    perform(range: nil)
  }

  private func perform(range: SoyehtProtocol.TextRange?) {
    guard !query.isEmpty else { return }
    pending?.cancel()
    generation += 1
    let token = generation
    let query = options.query(query)
    let replacement = replacement
    replacing = true
    pending = Task { [weak self, client, fileId] in
      defer { self?.replacing = false }
      do {
        let replaced = try await client.replaceInEditorFile(
          fileId, query: query, replacement: replacement, range: range)
        guard let self, self.generation == token else { return }
        let origin = replaced.cursor ?? range?.start ?? self.anchor
        self.anchor = origin
        try await self.find(query, from: origin, direction: .next, token: token)
      } catch is CancellationError {
      } catch {
        self?.fail(error, token: token)
      }
    }
  }

  private func search(from: TextPosition, direction: FileFindDirection, debounce: Bool) {
    pending?.cancel()
    generation += 1
    let token = generation
    guard !query.isEmpty else {
      result = nil
      problem = nil
      return
    }
    let query = options.query(query)
    pending = Task { [weak self] in
      do {
        if debounce { try await Task.sleep(for: .milliseconds(120)) }
        try await self?.find(query, from: from, direction: direction, token: token)
      } catch is CancellationError {
      } catch {
        self?.fail(error, token: token)
      }
    }
  }

  private func find(
    _ query: FileFindQuery, from: TextPosition, direction: FileFindDirection, token: Int
  ) async throws {
    let found = try await client.findInEditorFile(
      fileId, query: query, from: from, direction: direction)
    guard generation == token else { return }
    result = found
    problem = nil
    if let start = found.current?.range.start { anchor = start }
  }

  private func fail(_ error: Error, token: Int) {
    guard generation == token else { return }
    result = nil
    problem = error.localizedDescription
  }
}
