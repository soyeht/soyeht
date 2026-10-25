import SoyehtProtocol

/// Whether the engine could save a file, and why not (`contracts/vectors/editor.json`,
/// `saveStatus`; the desktop's `editor::save_status`). The engine tells it (`file.save_failed`,
/// and `saveFailure` on a document it read), so the pane shows what the engine found and never
/// a guess of its own.
public struct EditorSaveStatus: Sendable, Equatable {
  public private(set) var failure: EditorSaveFailure?

  public init() { failure = nil }

  /// The engine told that a save failed. Typing does not end it: only a save does, or the
  /// engine's word on a read.
  public mutating func told(_ reason: EditorSaveFailure) { failure = reason }

  /// The engine saved the file.
  public mutating func saved() { failure = nil }

  /// A page the engine read: a file with nothing to save has no failure.
  public mutating func read(_ page: FileReadResult) {
    failure = page.file.dirty ? page.saveFailure : nil
  }

  /// What the pane says about the failure, if there is one.
  public var text: String? { failure.map(Self.text(of:)) }

  public static func text(of reason: EditorSaveFailure) -> String {
    switch reason {
    case .missing: ClientPolicy.editorSaveMissing
    case .denied: ClientPolicy.editorSaveDenied
    case .full: ClientPolicy.editorSaveFull
    case .other: ClientPolicy.editorSaveOther
    }
  }
}
