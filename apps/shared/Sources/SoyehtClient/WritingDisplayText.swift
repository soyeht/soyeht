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

  public static func count(_ count: Int, kind: WritingCountKind) -> String {
    let names: (singular: String, plural: String)
    switch kind {
    case .word: names = ("word", "words")
    case .scene: names = ("scene", "scenes")
    case .slide: names = ("slide", "slides")
    }
    return "\(count) \(count == 1 ? names.singular : names.plural)"
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
