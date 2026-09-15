import Foundation
import SoyehtProtocol

public struct ScreenplayAdvanceEdit: Equatable, Sendable {
  public let range: NSRange
  public let replacement: String

  public init(range: NSRange, replacement: String) {
    self.range = range
    self.replacement = replacement
  }
}

public struct ScreenplayTypingEdit: Sendable {
  public let source: String
  public let cursor: Int
  public let activeLine: UInt32
  public let wireEdit: TextEdit

  public init(source: String, cursor: Int, activeLine: UInt32, wireEdit: TextEdit) {
    self.source = source
    self.cursor = cursor
    self.activeLine = activeLine
    self.wireEdit = wireEdit
  }
}

public enum ScreenplayInteraction {
  public static func typingEdit(
    source: String, firstLine: UInt32, range: NSRange, replacement: String
  ) -> ScreenplayTypingEdit? {
    let raw = source as NSString
    guard NSMaxRange(range) <= raw.length,
      let start = EditorEditing.textPosition(
        text: source, viewportStartLine: firstLine, utf16Offset: range.location),
      let end = EditorEditing.textPosition(
        text: source, viewportStartLine: firstLine, utf16Offset: NSMaxRange(range))
    else { return nil }
    let updated = raw.replacingCharacters(in: range, with: replacement)
    let cursor = range.location + replacement.utf16.count
    let line =
      EditorEditing.textPosition(
        text: updated, viewportStartLine: firstLine, utf16Offset: cursor)?.line ?? start.line
    return ScreenplayTypingEdit(
      source: updated, cursor: cursor, activeLine: line,
      wireEdit: TextEdit(range: .init(start: start, end: end), text: replacement))
  }

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

  public static func advanceEdit(
    source: String, cursor: Int, kind: ScreenplayElementKind, tab: Bool
  ) -> ScreenplayAdvanceEdit? {
    let raw = source as NSString
    guard cursor >= 0, cursor <= raw.length else { return nil }
    let lineRange = raw.lineRange(for: NSRange(location: cursor, length: 0))
    let contentRange = lineContentRange(lineRange, in: raw)
    let content = raw.substring(with: contentRange)
    let next = nextKind(after: kind, tab: tab)
    let seed = formattedLine("", as: next)
    if tab && content.trimmingCharacters(in: .whitespaces).isEmpty {
      return ScreenplayAdvanceEdit(range: contentRange, replacement: seed)
    }
    return ScreenplayAdvanceEdit(range: contentRange, replacement: content + "\n\n" + seed)
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

  private static func lineContentRange(_ lineRange: NSRange, in source: NSString) -> NSRange {
    var length = lineRange.length
    while length > 0 {
      let scalar = source.character(at: lineRange.location + length - 1)
      guard scalar == 10 || scalar == 13 else { break }
      length -= 1
    }
    return NSRange(location: lineRange.location, length: length)
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
