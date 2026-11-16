import Foundation

/// The agent's replies as the voice loop takes them: each message has an identity, grows by
/// updates, and comes out as the sentences to speak. The rules are in `replies` of
/// `contracts/vectors/speech-text.json`, which the Windows and Linux app passes too.
///
/// - A reply is a message: updates with the same `id` are one reply, and another `id` is another
///   reply, spoken after the one before it, whether or not a voice turn preceded it.
/// - A reply that was cancelled is never spoken again: updates that still arrive for its `id` are
///   dropped, so the late words of a cancelled answer are not taken for the next one.
public struct ReplyFeed: Sendable {
  private struct Reply: Sendable {
    var chunker: SentenceChunker
    var final = false
    var cancelled = false
  }

  /// How many identities are remembered, the oldest forgotten first.
  private static let remembered = 64

  private let template: SentenceChunker
  private var replies: [String: Reply] = [:]
  private var order: [String] = []

  public init(sentences: SentenceChunker = SentenceChunker()) { template = sentences }

  /// The sentences that became whole with this update of reply `id`.
  public mutating func update(id: String, markdown: String, final: Bool) -> [String] {
    var reply = replies[id] ?? Reply(chunker: template)
    guard !reply.cancelled else { return [] }
    let sentences = reply.chunker.update(
      SpeechText.speakable(markdown, streaming: !final), final: final)
    reply.final = reply.final || final
    if replies.updateValue(reply, forKey: id) == nil {
      order.append(id)
      if order.count > Self.remembered { replies[order.removeFirst()] = nil }
    }
    return sentences
  }

  /// Every reply seen so far is cancelled.
  public mutating func cancel() {
    for id in order { replies[id]?.cancelled = true }
  }

  /// Some reply that was not cancelled has more to come.
  public var hasPending: Bool { replies.values.contains { !$0.final && !$0.cancelled } }
}

/// The words of a reply the person heard before they talked over it.
public enum HeardSpeech {
  /// `played` are the sentences played to their end; `current` is the one that was playing, of
  /// which `fraction` (0 to 1) had been heard. With the voice's own word `timings`, a word is
  /// heard once the next one has started (the last one when all of the sentence was heard) at
  /// `position` seconds. Without them, the words that the fraction covers are kept, a word being
  /// a run of non-blank characters and each character of a script with no blanks a word of its
  /// own; the text is cut after the last word that is whole.
  public static func text(
    played: [String], current: String?, fraction: Double, timings: [SpeechTiming]? = nil,
    position: Double = 0
  ) -> String {
    let share = fraction.isNaN ? 0 : min(max(fraction, 0), 1)
    let partial: String
    if let words = timings?.filter({ $0.kind == .word }), !words.isEmpty {
      let heard = words.indices.filter { index in
        index + 1 < words.count ? words[index + 1].start <= position : share >= 1
      }
      partial = words.prefix(heard.count).map(\.text).joined(separator: " ")
    } else {
      partial = current.map { prefix(of: $0, share: share) } ?? ""
    }
    return (played + [partial]).filter { !$0.isEmpty }.joined(separator: " ")
  }

  private static func prefix(of text: String, share: Double) -> String {
    let scalars = Array(text.unicodeScalars)
    var ends: [Int] = []
    var index = 0
    while index < scalars.count {
      if scalars[index].properties.isWhitespace {
        index += 1
      } else if VoiceText.isBlankless(scalars[index]) {
        index += 1
        ends.append(index)
      } else {
        while index < scalars.count, !scalars[index].properties.isWhitespace,
          !VoiceText.isBlankless(scalars[index])
        {
          index += 1
        }
        ends.append(index)
      }
    }
    let count = Int((share * Double(ends.count)).rounded(.down))
    guard count > 0 else { return "" }
    var view = String.UnicodeScalarView()
    view.append(contentsOf: scalars[..<ends[count - 1]])
    return String(view).trimmingCharacters(in: .whitespaces)
  }
}
