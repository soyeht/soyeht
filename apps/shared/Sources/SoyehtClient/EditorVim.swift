import Foundation
import SoyehtProtocol

public enum EditorVimMode: String, Equatable, Sendable {
  case normal
  case insert
  case visual
}

public enum EditorVimKey: Equatable, Sendable {
  case character(String)
  case escape
  case enter
  case backspace
}

public enum EditorVimEffect: Equatable, Sendable {
  case save
  case close
  case reload
  case undo
  case substitute(pattern: String, replacement: String, everyMatch: Bool)
}

public struct EditorVimResult {
  public let handled: Bool
  public let plan: EditorInputPlan?
  public let selections: [NSRange]?
  public let effect: EditorVimEffect?

  init(
    handled: Bool = true, plan: EditorInputPlan? = nil, selections: [NSRange]? = nil,
    effect: EditorVimEffect? = nil
  ) {
    self.handled = handled
    self.plan = plan
    self.selections = selections
    self.effect = effect
  }
}

public struct EditorVimMachine {
  private enum Operator: String {
    case delete = "d"
    case change = "c"
    case yank = "y"
  }

  public private(set) var mode = EditorVimMode.normal
  public private(set) var commandLine: String?
  private var pendingOperator: Operator?
  private var waitingForSecondG = false
  private var visualAnchor: Int?
  private var visualCursor: Int?
  private var register = ""

  public init() {}

  public mutating func reset() {
    mode = .normal
    commandLine = nil
    pendingOperator = nil
    waitingForSecondG = false
    visualAnchor = nil
    visualCursor = nil
  }

  public mutating func handle(
    _ key: EditorVimKey, text: String, viewportStartLine: UInt32,
    selections: [NSRange], language: EditorLanguage
  ) -> EditorVimResult {
    if commandLine != nil { return command(key) }
    if key == .escape {
      reset()
      return EditorVimResult(selections: collapsedSelections(selections, text: text))
    }
    if mode == .insert { return EditorVimResult(handled: false) }
    guard case .character(let character) = key, character.utf16.count == 1 else {
      return EditorVimResult()
    }
    if mode == .visual {
      return visual(
        character, text: text, viewportStartLine: viewportStartLine,
        selections: selections, language: language)
    }
    return normal(
      character, text: text, viewportStartLine: viewportStartLine,
      selections: selections, language: language)
  }

  private mutating func normal(
    _ key: String, text: String, viewportStartLine: UInt32,
    selections: [NSRange], language: EditorLanguage
  ) -> EditorVimResult {
    let cursor = cursorOffset(selections, text: text)
    if let pendingOperator {
      self.pendingOperator = nil
      return operate(
        pendingOperator, key: key, cursor: cursor, text: text,
        viewportStartLine: viewportStartLine)
    }
    if waitingForSecondG {
      waitingForSecondG = false
      return key == "g" ? move(to: 0) : EditorVimResult()
    }
    if let operation = Operator(rawValue: key) {
      pendingOperator = operation
      return EditorVimResult()
    }
    switch key {
    case "h", "j", "k", "l", "w", "b", "0", "$", "G":
      return move(to: motion(key, from: cursor, in: text))
    case "g":
      waitingForSecondG = true
      return EditorVimResult()
    case "i":
      mode = .insert
      return EditorVimResult()
    case "a":
      mode = .insert
      return move(to: nextCharacter(after: cursor, in: text))
    case "I":
      mode = .insert
      return move(to: lineBounds(at: cursor, in: text).lowerBound)
    case "A":
      mode = .insert
      return move(to: lineBounds(at: cursor, in: text).upperBound)
    case "o", "O":
      return openLine(
        below: key == "o", cursor: cursor, text: text,
        viewportStartLine: viewportStartLine, language: language)
    case "v":
      mode = .visual
      visualAnchor = cursor
      visualCursor = cursor
      return EditorVimResult(selections: [visualRange(anchor: cursor, cursor: cursor, text: text)])
    case "x":
      return replace(
        range: nextRange(at: cursor, in: text), with: "", cursor: cursor, text: text,
        viewportStartLine: viewportStartLine)
    case "p":
      guard !register.isEmpty else { return EditorVimResult() }
      let target = nextCharacter(after: cursor, in: text)
      return replace(
        range: NSRange(location: target, length: 0), with: register,
        cursor: target + register.utf16.count - 1, text: text,
        viewportStartLine: viewportStartLine)
    case "u":
      return EditorVimResult(effect: .undo)
    case ":":
      commandLine = ":"
      return EditorVimResult()
    default:
      return EditorVimResult()
    }
  }

  private mutating func visual(
    _ key: String, text: String, viewportStartLine: UInt32,
    selections: [NSRange], language: EditorLanguage
  ) -> EditorVimResult {
    let cursor = visualCursor ?? cursorOffset(selections, text: text)
    if let operation = Operator(rawValue: key) {
      let range = selections.first ?? NSRange(location: cursor, length: 0)
      return apply(operation, range: range, text: text, viewportStartLine: viewportStartLine)
    }
    if key == "v" {
      reset()
      return move(to: cursor)
    }
    guard ["h", "j", "k", "l", "w", "b", "0", "$", "G"].contains(key) else {
      return EditorVimResult()
    }
    let target = motion(key, from: cursor, in: text)
    let anchor = visualAnchor ?? cursor
    visualCursor = target
    return EditorVimResult(selections: [visualRange(anchor: anchor, cursor: target, text: text)])
  }

  private mutating func operate(
    _ operation: Operator, key: String, cursor: Int, text: String,
    viewportStartLine: UInt32
  ) -> EditorVimResult {
    let range: NSRange
    if key == operation.rawValue {
      range = wholeLine(at: cursor, in: text)
    } else if ["h", "j", "k", "l", "w", "b", "0", "$", "G"].contains(key) {
      let target = motion(key, from: cursor, in: text)
      range = operatorRange(from: cursor, to: target, motion: key, text: text)
    } else {
      return EditorVimResult()
    }
    return apply(operation, range: range, text: text, viewportStartLine: viewportStartLine)
  }

  private mutating func apply(
    _ operation: Operator, range: NSRange, text: String, viewportStartLine: UInt32
  ) -> EditorVimResult {
    guard range.length > 0, NSMaxRange(range) <= text.utf16.count else {
      return EditorVimResult()
    }
    register = (text as NSString).substring(with: range)
    if operation == .yank {
      reset()
      return move(to: range.location)
    }
    if operation == .change {
      mode = .insert
      visualAnchor = nil
      visualCursor = nil
    } else {
      reset()
    }
    return replace(
      range: range, with: "", cursor: range.location, text: text,
      viewportStartLine: viewportStartLine)
  }

  private mutating func openLine(
    below: Bool, cursor: Int, text: String, viewportStartLine: UInt32,
    language: EditorLanguage
  ) -> EditorVimResult {
    let bounds = lineBounds(at: cursor, in: text)
    let insertion = below ? bounds.upperBound : bounds.lowerBound
    let prefix = lineIndent(at: cursor, in: text)
    let inserted = below ? "\n\(prefix)" : "\(prefix)\n"
    let newCursor = below ? insertion + inserted.utf16.count : insertion + prefix.utf16.count
    mode = .insert
    return replace(
      range: NSRange(location: insertion, length: 0), with: inserted, cursor: newCursor,
      text: text, viewportStartLine: viewportStartLine)
  }

  private mutating func command(_ key: EditorVimKey) -> EditorVimResult {
    guard var commandLine else { return EditorVimResult() }
    switch key {
    case .escape:
      self.commandLine = nil
      return EditorVimResult()
    case .backspace:
      if commandLine.count > 1 { commandLine.removeLast() } else { self.commandLine = nil }
      self.commandLine = commandLine.count > 1 ? commandLine : nil
      return EditorVimResult()
    case .enter:
      self.commandLine = nil
      return EditorVimResult(effect: parseCommand(commandLine))
    case .character(let value):
      guard commandLine.utf8.count + value.utf8.count <= 4096 else { return EditorVimResult() }
      commandLine += value
      self.commandLine = commandLine
      return EditorVimResult()
    }
  }

  private func parseCommand(_ command: String) -> EditorVimEffect? {
    switch command {
    case ":w": return .save
    case ":q": return .close
    case ":e": return .reload
    default: return parseSubstitution(command)
    }
  }

  private func parseSubstitution(_ command: String) -> EditorVimEffect? {
    let parts = separated(command.dropFirst(4))
    guard command.hasPrefix(":%s/"), parts.count >= 2,
      !parts[0].isEmpty
    else { return nil }
    let everyMatch = parts.count == 3 && parts[2] == "g"
    guard parts.count == 2 || (parts.count == 3 && (parts[2].isEmpty || everyMatch)) else {
      return nil
    }
    return .substitute(pattern: parts[0], replacement: parts[1], everyMatch: everyMatch)
  }

  private func separated(_ source: Substring) -> [String] {
    var parts = [""]
    var escaped = false
    for character in source {
      if escaped {
        parts[parts.count - 1].append(character)
        escaped = false
      } else if character == "\\" {
        escaped = true
      } else if character == "/" {
        parts.append("")
      } else {
        parts[parts.count - 1].append(character)
      }
    }
    if escaped { parts[parts.count - 1].append("\\") }
    return parts
  }

  private func motion(_ key: String, from cursor: Int, in text: String) -> Int {
    switch key {
    case "h": return previousCharacter(before: cursor, in: text)
    case "l": return nextCharacter(after: cursor, in: text)
    case "j": return vertical(from: cursor, delta: 1, in: text)
    case "k": return vertical(from: cursor, delta: -1, in: text)
    case "w": return nextWord(from: cursor, in: text)
    case "b": return previousWord(from: cursor, in: text)
    case "0": return lineBounds(at: cursor, in: text).lowerBound
    case "$": return lineBounds(at: cursor, in: text).upperBound
    case "G": return max(0, text.utf16.count - 1)
    default: return cursor
    }
  }

  private func move(to offset: Int) -> EditorVimResult {
    EditorVimResult(selections: [NSRange(location: max(0, offset), length: 0)])
  }

  private func replace(
    range: NSRange, with replacement: String, cursor: Int, text: String,
    viewportStartLine: UInt32
  ) -> EditorVimResult {
    let target = min(max(0, cursor), text.utf16.count - range.length + replacement.utf16.count)
    let plan = EditorEditing.replacementPlan(
      text: text, viewportStartLine: viewportStartLine,
      replacements: [EditorLocalReplacement(range: range, text: replacement)],
      selections: [NSRange(location: target, length: 0)])
    return EditorVimResult(plan: plan)
  }

  private func cursorOffset(_ selections: [NSRange], text: String) -> Int {
    min(selections.first?.location ?? 0, text.utf16.count)
  }

  private func collapsedSelections(_ selections: [NSRange], text: String) -> [NSRange] {
    [NSRange(location: cursorOffset(selections, text: text), length: 0)]
  }

  private func nextRange(at cursor: Int, in text: String) -> NSRange {
    let source = text as NSString
    guard cursor < source.length else { return NSRange(location: cursor, length: 0) }
    return source.rangeOfComposedCharacterSequence(at: cursor)
  }

  private func nextCharacter(after cursor: Int, in text: String) -> Int {
    let range = nextRange(at: cursor, in: text)
    return min(text.utf16.count, NSMaxRange(range))
  }

  private func previousCharacter(before cursor: Int, in text: String) -> Int {
    let source = text as NSString
    guard cursor > 0 else { return 0 }
    return source.rangeOfComposedCharacterSequence(at: cursor - 1).location
  }

  private func lineBounds(at cursor: Int, in text: String) -> Range<Int> {
    let source = text as NSString
    let location = min(max(0, cursor), source.length)
    let raw = source.lineRange(for: NSRange(location: location, length: 0))
    let end =
      raw.length > 0 && NSMaxRange(raw) <= source.length
        && source.character(at: NSMaxRange(raw) - 1) == 10 ? NSMaxRange(raw) - 1 : NSMaxRange(raw)
    return raw.location..<end
  }

  private func wholeLine(at cursor: Int, in text: String) -> NSRange {
    let source = text as NSString
    let raw = source.lineRange(for: NSRange(location: min(cursor, source.length), length: 0))
    return raw
  }

  private func vertical(from cursor: Int, delta: Int, in text: String) -> Int {
    let source = text as NSString
    let starts = lineStarts(source)
    let index = starts.lastIndex(where: { $0 <= cursor }) ?? 0
    let targetIndex = min(max(0, index + delta), starts.count - 1)
    let column = cursor - starts[index]
    let targetEnd = lineBounds(at: starts[targetIndex], in: text).upperBound
    return min(starts[targetIndex] + column, targetEnd)
  }

  private func nextWord(from cursor: Int, in text: String) -> Int {
    let source = text as NSString
    var offset = min(cursor, source.length)
    if isWord(at: offset, in: source) {
      while offset < source.length, isWord(at: offset, in: source) {
        offset = nextCharacter(after: offset, in: text)
      }
    }
    while offset < source.length, !isWord(at: offset, in: source) {
      offset = nextCharacter(after: offset, in: text)
    }
    return offset
  }

  private func previousWord(from cursor: Int, in text: String) -> Int {
    let source = text as NSString
    var offset = previousCharacter(before: cursor, in: text)
    while offset > 0, !isWord(at: offset, in: source) {
      offset = previousCharacter(before: offset, in: text)
    }
    while offset > 0 {
      let previous = previousCharacter(before: offset, in: text)
      if !isWord(at: previous, in: source) { break }
      offset = previous
    }
    return offset
  }

  private func isWord(at offset: Int, in source: NSString) -> Bool {
    guard offset < source.length else { return false }
    let range = source.rangeOfComposedCharacterSequence(at: offset)
    let value = source.substring(with: range)
    return value == "_" || value.rangeOfCharacter(from: .alphanumerics) != nil
  }

  private func operatorRange(from cursor: Int, to target: Int, motion: String, text: String)
    -> NSRange
  {
    let start = min(cursor, target)
    var end = max(cursor, target)
    if motion == "l", target >= cursor { end = nextCharacter(after: target, in: text) }
    return NSRange(location: start, length: max(0, end - start))
  }

  private func visualRange(anchor: Int, cursor: Int, text: String) -> NSRange {
    let start = min(anchor, cursor)
    let last = max(anchor, cursor)
    let end = nextCharacter(after: last, in: text)
    return NSRange(location: start, length: max(0, end - start))
  }

  private func lineStarts(_ source: NSString) -> [Int] {
    var result = [0]
    for offset in 0..<source.length where source.character(at: offset) == 10 {
      result.append(offset + 1)
    }
    return result
  }

  private func lineIndent(at cursor: Int, in text: String) -> String {
    let source = text as NSString
    let start = lineBounds(at: cursor, in: text).lowerBound
    var end = start
    while end < source.length, [9, 32].contains(source.character(at: end)) { end += 1 }
    return source.substring(with: NSRange(location: start, length: end - start))
  }
}

public enum EditorVimSubstitution {
  public static func edits(
    lines: [EditorLine], pattern: String, replacement: String, everyMatch: Bool,
    remainingLimit: Int
  ) -> [TextEdit]? {
    guard !pattern.isEmpty, remainingLimit >= 0 else { return nil }
    var result: [TextEdit] = []
    for line in lines {
      let source = line.text as NSString
      var search = NSRange(location: 0, length: source.length)
      while search.length > 0 {
        let match = source.range(of: pattern, options: [], range: search)
        guard match.location != NSNotFound else { break }
        guard result.count < remainingLimit,
          let start = UInt32(exactly: match.location),
          let end = UInt32(exactly: NSMaxRange(match))
        else { return nil }
        result.append(
          TextEdit(
            range: TextRange(
              start: TextPosition(line: line.line, column: start),
              end: TextPosition(line: line.line, column: end)),
            text: replacement))
        if !everyMatch { break }
        let next = NSMaxRange(match)
        search = NSRange(location: next, length: source.length - next)
      }
    }
    return result
  }
}
