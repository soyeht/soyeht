import Foundation

/// What the New document sheet holds: a title and an optional subfolder. The button and the
/// Return key in either field share one rule, so neither can create a blank or a second document.
public struct WritingDocumentDraft: Equatable, Sendable {
  public var title = ""
  public var folder = ""

  public init(title: String = "", folder: String = "") {
    self.title = title
    self.folder = folder
  }

  /// What to create, or `nil` while the title is blank or a creation is already running.
  public func submission(creating: Bool) -> Submission? {
    let title = title.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !creating, !title.isEmpty else { return nil }
    let folder = folder.trimmingCharacters(in: .whitespacesAndNewlines)
    return Submission(title: title, relativeFolder: folder.isEmpty ? nil : folder)
  }

  public struct Submission: Equatable, Sendable {
    public let title: String
    public let relativeFolder: String?
  }
}
