import Foundation
import SoyehtProtocol

/// What an editor pane knows of its file's diff against HEAD. The text never waits for it: the
/// diff is asked for only once a page is held, and the gutter shows it when it arrives.
public enum EditorDiffState: Sendable {
  case none
  case loading(revision: String)
  case ready(FileDiffResult)
  case failed(revision: String, reason: String)

  /// The revision this state was asked for or answers; nil before any ask.
  var revision: String? {
    switch self {
    case .none: nil
    case .loading(let revision): revision
    case .ready(let result): result.revision
    case .failed(let revision, _): revision
    }
  }
}

extension EditorFileView {
  /// The diff of the revision the pane shows, once it has arrived.
  public var readyDiff: FileDiffResult? {
    guard case .ready(let result) = diff, result.revision == file.revision else { return nil }
    return result
  }

  /// Why the diff of the revision the pane shows could not be read.
  public var diffProblem: String? {
    guard case .failed(let revision, let reason) = diff, revision == file.revision else {
      return nil
    }
    return reason
  }
}

extension EngineClient {
  /// How long a revision stays unchanged before its diff is read: typing, or a file that the
  /// person is passing over, asks for none.
  static let editorDiffSettleMilliseconds = 120

  /// Reads the diff of a file's current revision, once per revision. Nothing is asked before the
  /// file's first page is held, and a revision that was asked for (or answered, or refused) is
  /// not asked again when a pane appears again. A cancelled caller leaves no mark, so the next
  /// one asks; a request that left already ends in the replica, whoever waits for it.
  public func loadEditorDiff(fileId: String) async {
    guard wantsEditorDiff(fileId) != nil else { return }
    do {
      try await Task.sleep(for: .milliseconds(Self.editorDiffSettleMilliseconds))
    } catch { return }
    guard let revision = wantsEditorDiff(fileId) else { return }
    updateEditor(fileId) { $0.diff = .loading(revision: revision) }
    do {
      let result = try await editorDiff(fileId: fileId, revision: revision)
      updateEditor(fileId) {
        if $0.file.revision == result.revision {
          $0.diff = .ready(result)
        } else if $0.diff.revision == revision {
          $0.diff = .none
        }
      }
    } catch {
      updateEditor(fileId) {
        if $0.diff.revision == revision {
          $0.diff = .failed(revision: revision, reason: error.localizedDescription)
        }
      }
    }
  }

  /// The revision whose diff should be asked for now, or nil when its page is not held yet or
  /// the revision was already asked for.
  private func wantsEditorDiff(_ fileId: String) -> String? {
    guard let held = editorFiles[fileId], held.viewport != nil,
      held.diff.revision != held.file.revision
    else { return nil }
    return held.file.revision
  }
}
