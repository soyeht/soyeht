import SoyehtProtocol

/// How every client names the three kinds of writing document.
extension WritingSection {
  public var title: String {
    switch self {
    case .texts: "Texts"
    case .presentations: "Presentations"
    case .screenplays: "Screenplays"
    }
  }

  public var singularTitle: String {
    switch self {
    case .texts: "text"
    case .presentations: "presentation"
    case .screenplays: "screenplay"
    }
  }

  public var icon: String {
    switch self {
    case .texts: "text.page"
    case .presentations: "rectangle.on.rectangle"
    case .screenplays: "movieclapper"
    }
  }

  /// The line under the section's title.
  public var subtitle: String {
    switch self {
    case .texts: "Essays, notes, and long-form Markdown"
    case .presentations: "Slide documents with speaker notes"
    case .screenplays: "Fountain scripts and scene plans"
    }
  }

  /// The folder of the workspace that holds the section's documents.
  public var folderLabel: String {
    switch self {
    case .texts: "texts"
    case .presentations: "presentations"
    case .screenplays: "screenplays"
    }
  }
}
