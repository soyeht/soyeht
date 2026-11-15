import Foundation

/// Cuts a text into pieces the Kokoro model can speak in one pass. The model takes at most 128
/// phoneme tokens and renders at most five seconds of audio, and it cuts what is longer without
/// a word, so a text is split before it gets there: at the end of a sentence first, then between
/// words.
enum SpeechTextChunks {
  /// What one pass holds at the normal pace: about five seconds of speech.
  static let charactersAtNormalPace = 60
  private static let minimum = 30
  private static let maximum = 100

  /// The size of a piece for a pace: a slower voice fits fewer characters in five seconds.
  static func limit(forSpeed speed: Double) -> Int {
    max(minimum, min(maximum, Int(Double(charactersAtNormalPace) * speed)))
  }

  static func split(_ text: String, maxCharacters: Int) -> [String] {
    var chunks: [String] = []
    var current = ""
    for word in text.split(whereSeparator: \.isWhitespace) {
      for piece in pieces(of: word, maxCharacters) {
        let joined = current.isEmpty ? piece : current + " " + piece
        if joined.count > maxCharacters {
          chunks.append(current)
          current = piece
        } else {
          current = joined
        }
      }
      if endsSentence(current) {
        chunks.append(current)
        current = ""
      }
    }
    if !current.isEmpty { chunks.append(current) }
    return chunks
  }

  /// A word longer than a piece is cut into pieces of the limit.
  private static func pieces(of word: Substring, _ limit: Int) -> [String] {
    var rest = word
    var pieces: [String] = []
    while rest.count > limit {
      pieces.append(String(rest.prefix(limit)))
      rest = rest.dropFirst(limit)
    }
    pieces.append(String(rest))
    return pieces
  }

  private static func endsSentence(_ text: String) -> Bool {
    guard let last = text.last else { return false }
    return ".!?…".contains(last)
  }
}
