import Foundation
import SoyehtProtocol

public enum WritingCountKind: Equatable, Sendable {
  case word
  case scene
  case slide
}

public enum WritingDisplayText {
  public static let screenplayElementKinds: [ScreenplayElementKind] = [
    .scene_heading, .action, .character, .parenthetical, .dialogue, .transition, .shot,
  ]

  /// The fonts a text document can use, in the order its font menu lists them.
  public static let fonts: [WritingFont] = [.reading_serif, .classic_serif, .sans]

  public static func count(_ count: Int, kind: WritingCountKind) -> String {
    let names: (singular: String, plural: String)
    switch kind {
    case .word: names = ("word", "words")
    case .scene: names = ("scene", "scenes")
    case .slide: names = ("slide", "slides")
    }
    return "\(count) \(count == 1 ? names.singular : names.plural)"
  }

  /// The name a writing pane's header shows: the last part of the document's path, else the
  /// pane's own name while the pane has no document yet.
  public static func fileName(path: String?, fallback: String) -> String {
    guard let path else { return fallback }
    return URL(fileURLWithPath: path).lastPathComponent
  }

  /// What a section's list says when it shows no document: the heading, and under it the line
  /// that tells where documents are kept (not for a search that found nothing).
  public static func emptySection(
    _ section: WritingSection, searching: Bool
  ) -> (heading: String, detail: String?) {
    guard !searching else { return ("No matching documents", nil) }
    return (
      "No documents yet", "Create one here and it stays in the \(section.folderLabel) folder."
    )
  }

  /// The title of the form that makes a document of the section.
  public static func newDocumentTitle(_ section: WritingSection) -> String {
    "New \(section.singularTitle)"
  }

  /// The line under the new-document form's fields.
  public static func newDocumentNote(_ section: WritingSection) -> String {
    "The document is created inside the \(section.folderLabel) folder."
  }

  public static func fontName(_ font: WritingFont) -> String {
    switch font {
    case .reading_serif: "Newsreader"
    case .classic_serif: "Classic serif"
    case .sans: "Sans"
    }
  }

  public static func slideTitle(_ heading: String) -> String {
    heading.isEmpty ? "Untitled slide" : heading
  }

  /// What a slide shows under its title in the rail: its notes, or that it has none.
  public static func slideNotes(_ notes: String) -> String {
    notes.isEmpty ? "No speaker notes" : notes
  }

  /// What the source of a presentation means, in the line under it.
  public static let presentationHint =
    "Title → appears on slide · regular text → your notes · --- → new slide"

  /// What the source of a presentation is written in, at the end of that line.
  public static let presentationFormat = "Markdown"

  public static func sceneTitle(_ heading: String) -> String {
    heading.isEmpty ? "Untitled scene" : heading
  }

  public static func screenplayElementName(_ kind: ScreenplayElementKind) -> String {
    switch kind {
    case .scene_heading: "Scene heading"
    case .action: "Action"
    case .character: "Character"
    case .parenthetical: "Parenthetical"
    case .dialogue: "Dialogue"
    case .transition: "Transition"
    case .shot: "Shot"
    }
  }
}
