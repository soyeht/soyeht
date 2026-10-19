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
}
