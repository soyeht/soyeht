import Foundation
import SoyehtProtocol

/// Cuts the speakable text of a reply that is still arriving into the sentences a voice speaks,
/// the first as early as it is whole. The rules are in `contracts/vectors/speech-text.json`, which
/// the Windows and Linux app passes too; lengths count Unicode scalar values, a blank is a scalar
/// with the Unicode `White_Space` property, and the stops, closers, abbreviations and limits are
/// `x-clientPolicy.voice` (`sentence...`).
///
/// - A sentence ends at a stop of `sentenceStops` (and any closer of `sentenceClosers` after it) when
///   a blank follows, or at the end of the line when the line is finished; at a stop of
///   `sentenceWideStops` at once. A full stop does not end a sentence in a number, after an
///   abbreviation of `sentenceAbbreviations`, or when the next word starts in lowercase ("5 p.m.
///   today", "etc. and").
/// - A line ends at a newline. Sentences of two lines are never joined.
/// - A sentence shorter than `minimumLength` waits for the next one of its line, so a voice is not
///   asked for "Yes." alone; a line's last sentence is spoken as it is once the line is finished.
/// - A sentence longer than `maximumLength` is cut after its last `,`, `;` or `:` that has a blank
///   after it before the limit, else at its last blank, else at the limit.
/// - When the text is not an extension of the one before, cutting goes back to the end of the last
///   sentence that both texts begin with.
public struct SentenceChunker: Sendable {
  public let minimumLength: Int
  public let maximumLength: Int
  private var consumed = 0
  private var previous: [Unicode.Scalar] = []
  private var boundaries: [Int] = []

  static let abbreviations = Set(
    ClientPolicy.voiceSentenceAbbreviations.split(separator: "|").map(String.init))
  private static let stops = Set(ClientPolicy.voiceSentenceStops.unicodeScalars)
  private static let wideStops = Set(ClientPolicy.voiceSentenceWideStops.unicodeScalars)
  private static let closers = Set(ClientPolicy.voiceSentenceClosers.unicodeScalars)

  public init(
    minimumLength: Int = ClientPolicy.voiceSentenceMinChars,
    maximumLength: Int = ClientPolicy.voiceSentenceMaxChars
  ) {
    self.minimumLength = minimumLength
    self.maximumLength = maximumLength
  }

  /// The sentences that became whole with this text: `spoken` is all the speakable text so far,
  /// and `final` says the reply is over, so what is left is spoken as it is.
  public mutating func update(_ spoken: String, final: Bool) -> [String] {
    let scalars = Array(spoken.unicodeScalars)
    resynchronize(with: scalars)
    var emitted: [String] = []
    var lineStart = min(consumed, scalars.count)
    while lineStart <= scalars.count {
      let newline = scalars[lineStart...].firstIndex(of: "\n")
      let closed = newline != nil || final
      let end = newline ?? scalars.count
      let line = Array(scalars[lineStart..<end])
      let (pieces, leftover, ends) = Self.pieces(
        of: line, closed: closed, minimum: minimumLength, maximum: maximumLength)
      emitted += pieces
      boundaries += ends.map { lineStart + $0 }
      guard let newline else {
        consumed = lineStart + (closed ? line.count : leftover)
        break
      }
      lineStart = newline + 1
      consumed = lineStart
      boundaries.append(consumed)
    }
    return emitted
  }

  /// A text that no longer begins with what was cut goes back to the last sentence end both share.
  private mutating func resynchronize(with scalars: [Unicode.Scalar]) {
    let shared = zip(previous, scalars).prefix { $0 == $1 }.count
    previous = scalars
    guard shared < consumed else { return }
    consumed = boundaries.last { $0 <= shared } ?? 0
    boundaries.removeAll { $0 > consumed }
  }

  /// The sentences of one line that can be spoken now, and how many scalars of the line they
  /// cover (all of it when the line is finished).
  private static func pieces(
    of line: [Unicode.Scalar], closed: Bool, minimum: Int, maximum: Int
  ) -> (pieces: [String], covered: Int, ends: [Int]) {
    var units: [(text: String, end: Int)] = []
    var start = 0
    var index = 0
    while index < line.count {
      guard let end = sentenceEnd(in: line, at: index, closed: closed) else {
        index += 1
        continue
      }
      units.append((text(line[start..<end]), end))
      start = end
      index = end
    }
    let tail = start
    if closed { units.append((text(line[tail...]), line.count)) }
    units.removeAll { $0.text.isEmpty }

    var pieces: [String] = []
    var ends: [Int] = []
    var pending = ""
    var covered = 0
    for (position, unit) in units.enumerated() {
      pending = join(pending, unit.text)
      guard pending.unicodeScalars.count >= minimum || (closed && position == units.count - 1)
      else { continue }
      pieces += cut(Array(pending.unicodeScalars), maximum: maximum).map(\.text)
      pending = ""
      covered = unit.end
      ends.append(covered)
    }
    if closed { return (pieces, line.count, ends) }
    // An unfinished tail that has outgrown the limit is spoken in parts; the rest waits.
    if pending.isEmpty {
      let parts = cut(Array(line[tail...]), maximum: maximum)
      for part in parts.dropLast() {
        pieces.append(part.text)
        covered = tail + part.end
        ends.append(covered)
      }
    }
    return (pieces, covered, ends)
  }

  /// The two pieces as one sentence: a blank between them, except after a stop of a script that
  /// writes no blanks.
  private static func join(_ first: String, _ second: String) -> String {
    guard let last = first.unicodeScalars.last else { return second }
    return first + (wideStops.contains(last) ? "" : " ") + second
  }

  /// Where the sentence that stops at `index` ends in `line` (after its stops and closers), or nil.
  private static func sentenceEnd(in line: [Unicode.Scalar], at index: Int, closed: Bool) -> Int? {
    let scalar = line[index]
    guard stops.contains(scalar) || wideStops.contains(scalar) else { return nil }
    var end = index + 1
    while end < line.count, stops.contains(line[end]) || wideStops.contains(line[end]) { end += 1 }
    while end < line.count, closers.contains(line[end]) { end += 1 }
    if wideStops.contains(scalar) { return end }
    if end < line.count {
      guard line[end].properties.isWhitespace else { return nil }
    } else if !closed {
      return nil
    }
    guard scalar == "." else { return end }
    if end == index + 1, isAbbreviation(line[...index]) { return nil }
    // A full stop followed by a word in lowercase does not end the sentence; one with nothing
    // after it yet may still be followed by one.
    guard let next = line[end...].first(where: { !$0.properties.isWhitespace }) else {
      return closed ? end : nil
    }
    return next.properties.isLowercase ? nil : end
  }

  private static func isAbbreviation(_ upToStop: ArraySlice<Unicode.Scalar>) -> Bool {
    let word = upToStop.reversed().prefix { !$0.properties.isWhitespace }.reversed()
    var view = String.UnicodeScalarView()
    view.append(contentsOf: word)
    return abbreviations.contains(String(view).lowercased())
  }

  private static func text(_ scalars: ArraySlice<Unicode.Scalar>) -> String {
    var view = String.UnicodeScalarView()
    view.append(contentsOf: scalars)
    return String(view).trimmingCharacters(in: .whitespaces)
  }

  /// `scalars` as one piece, or several when it is longer than `maximum`; each part with where in
  /// `scalars` it stops.
  private static func cut(_ scalars: [Unicode.Scalar], maximum: Int) -> [(text: String, end: Int)] {
    var parts: [(text: String, end: Int)] = []
    var from = 0
    while scalars.count - from > maximum {
      let window = from..<(from + maximum)
      let clause = window.last { index in
        [",", ";", ":"].contains(scalars[index]) && scalars[index + 1].properties.isWhitespace
      }
      let at =
        clause.map { $0 + 1 } ?? window.last { scalars[$0].properties.isWhitespace }
        ?? window.upperBound
      parts.append((text(scalars[from..<at]), at))
      from = at
      while from < scalars.count, scalars[from].properties.isWhitespace { from += 1 }
    }
    let last = text(scalars[from...])
    return last.isEmpty ? parts : parts + [(last, scalars.count)]
  }
}
