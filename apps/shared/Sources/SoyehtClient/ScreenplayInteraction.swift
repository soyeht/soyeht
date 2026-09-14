import Foundation
import SoyehtProtocol

public enum ScreenplayInteraction {
  public static func element(
    at line: UInt32, in structure: WritingStructureResult
  ) -> ScreenplayElement? {
    structure.elements.first { line >= $0.startLine && line <= $0.endLine }
  }

  public static func variants(
    at line: UInt32, in structure: WritingStructureResult
  ) -> WritingLineVariants? {
    structure.variants.first { $0.line == line }
  }

  public static func nextKind(
    after kind: ScreenplayElementKind, tab: Bool
  ) -> ScreenplayElementKind {
    switch (kind, tab) {
    case (.scene_heading, _): .action
    case (.character, _), (.parenthetical, _): .dialogue
    case (.dialogue, true): .parenthetical
    case (.dialogue, false): .character
    case (.action, true): .character
    case (.transition, _), (.shot, _): .scene_heading
    default: .action
    }
  }

  public static func formattedLine(
    _ source: String, as kind: ScreenplayElementKind
  ) -> String {
    let text = source.trimmingCharacters(in: .whitespacesAndNewlines)
    switch kind {
    case .scene_heading:
      return text.isEmpty ? ".INT. " : "." + unforced(text).uppercased()
    case .character:
      return text.isEmpty ? "@" : "@" + unforced(text).uppercased()
    case .parenthetical:
      return "(" + text.trimmingCharacters(in: CharacterSet(charactersIn: "()")) + ")"
    case .transition:
      return ">" + unforced(text).uppercased()
    case .shot:
      let upper = unforced(text).uppercased()
      return upper.hasSuffix(":") ? upper : upper + ":"
    case .action, .dialogue:
      return unforced(text)
    }
  }

  public static func suggestions(
    for kind: ScreenplayElementKind, prefix: String, in structure: WritingStructureResult
  ) -> [String] {
    let candidates = candidates(for: kind, structure: structure)
    let needle = unforced(prefix).uppercased()
    return candidates.filter { needle.isEmpty || $0.uppercased().hasPrefix(needle) }
  }

  public static func adjacentVariant(
    in variants: WritingLineVariants, delta: Int
  ) -> WritingVariant? {
    guard !variants.alternates.isEmpty else { return nil }
    let count = variants.alternates.count
    let selected = min(Int(variants.selectedIndex), count - 1)
    let next = (selected + delta % count + count) % count
    return variants.alternates[next]
  }

  private static func candidates(
    for kind: ScreenplayElementKind, structure: WritingStructureResult
  ) -> [String] {
    switch kind {
    case .transition:
      return ["CUT TO:", "DISSOLVE TO:", "FADE IN:", "FADE OUT:"]
    case .character:
      return unique(
        structure.elements.filter { $0.kind == .character }.map { characterName($0.text) })
    case .parenthetical:
      return ["(V.O.)", "(O.S.)", "(CONT'D)"]
    case .scene_heading:
      return unique(structure.scenes.map { sceneLocation($0.heading) })
    default:
      return []
    }
  }

  private static func unique(_ values: [String]) -> [String] {
    var seen = Set<String>()
    return values.filter { !$0.isEmpty && seen.insert($0).inserted }
  }

  private static func unforced(_ value: String) -> String {
    value.trimmingCharacters(in: CharacterSet(charactersIn: ".@><"))
      .trimmingCharacters(in: .whitespaces)
  }

  private static func characterName(_ value: String) -> String {
    value.components(separatedBy: " (").first?.trimmingCharacters(in: .whitespaces) ?? value
  }

  private static func sceneLocation(_ value: String) -> String {
    let withoutPrefix = value.split(separator: ".", maxSplits: 1).last.map(String.init) ?? value
    return withoutPrefix.components(separatedBy: " - ").first?
      .trimmingCharacters(in: .whitespaces) ?? withoutPrefix
  }
}
