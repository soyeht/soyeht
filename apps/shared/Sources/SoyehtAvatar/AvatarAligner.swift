import Foundation

/// The result of aligning a text to a clip.
struct AvatarAlignment: Equatable, Sendable {
  /// The phonemes on the clip's time, ready for `AvatarClip`. Empty when the audio has no voice
  /// or the text no sound.
  var phonemes: [AvatarPhoneme] = []
  /// The voiced span of the audio, in seconds.
  var span: (start: Double, end: Double)?
  /// How many boundaries between words were paired with a silence of the audio, out of
  /// `boundaries`.
  var matched = 0
  var boundaries = 0
  /// When each word of the text starts, for the accents of the acting layer.
  var words: [AvatarWordStart] = []
  /// False when the text has more sounds than speech can hold in this audio: the text does not
  /// belong to the audio, and the phonemes are left empty, so the runtime follows the level of
  /// the voice.
  var fits = true

  static func == (a: Self, b: Self) -> Bool {
    a.phonemes == b.phonemes && a.span?.start == b.span?.start && a.span?.end == b.span?.end
      && a.matched == b.matched && a.boundaries == b.boundaries && a.words == b.words
      && a.fits == b.fits
  }
}

/// The audio-to-mouth aligner (tier 2 of ADR 0166, section 6): the text of a reply and the
/// envelope of the audio that actually plays become a timed phoneme track, for a TTS that gives no
/// timings.
///
/// 1. The text becomes phonemes by our own letter rules (`AvatarLetters`).
/// 2. The envelope gives the voiced span of the clip and the silences inside it.
/// 3. Each word gets its share of the voiced time in proportion to its sounds; the boundaries the
///    text predicts are matched to the silences the audio shows, and the words are stretched to
///    fit between them. Pause marks take the span of the silence they met.
///
/// This uses the voiced span and the silences only. It does not listen to the vowels: that would
/// be a trained model, which the ADR leaves out of the launch.
enum AvatarAligner {
  /// The most sounds a second of speech holds. The lab's real clips have 13 to 14 a second, fast
  /// speech about twice that; a text with more than this in the audio it is placed on is not that
  /// audio's text.
  private static let maxSoundsPerSecond = 30.0
  /// The share of a word's time that a consonant takes beside a vowel's 1.
  private static let consonantWeight = 0.55
  /// Tolerance for comparing a warped position with a silence's position (seconds).
  private static let epsilon = 1e-9

  private struct Word {
    var text: String
    var phonemes: [Unicode.Scalar]
    var weight: Double
    var pause: Unicode.Scalar?
  }

  private static func weight(_ p: Unicode.Scalar) -> Double {
    if p == AvatarPhonemes.stress { return 0 }
    return AvatarPhonemes.vowel(p) != nil ? 1 : consonantWeight
  }

  private static func words(_ text: String, language: AvatarLanguage) -> [Word] {
    var out: [Word] = []
    for token in AvatarLetters.tokens(text) {
      switch token {
      case .word(let w):
        let phonemes = AvatarLetters.phonemize(w, language: language)
        let total = phonemes.reduce(0) { $0 + weight($1) }
        if total > 0 { out.append(Word(text: w, phonemes: phonemes, weight: total, pause: nil)) }
      case .pause(let p):
        if let last = out.indices.last, out[last].pause == nil { out[last].pause = p }
      }
    }
    return out
  }

  /// The silences of the voiced span, with where each one sits in voiced time (the time with the
  /// silences taken out) and how much silence lies before it.
  private struct Valleys {
    var all: [AvatarValley]
    var positions: [Double]
    /// `before[k]` is the total duration of the first `k` silences.
    var before: [Double]

    init(spanStart: Double, valleys: [AvatarValley]) {
      var positions: [Double] = []
      var before = [0.0]
      for v in valleys {
        let gone = before.last ?? 0
        positions.append((v.start - spanStart) - gone)
        before.append(gone + v.duration)
      }
      all = valleys
      self.positions = positions
      self.before = before
    }

    /// Voiced time to clip time: the silences before the point are put back. `after` says
    /// whether a silence that starts exactly at the point is before it (the start of the next
    /// word) or after it (the end of the word).
    func toClip(spanStart: Double, y: Double, after: Bool) -> Double {
      let count =
        after
        ? positions.partitionPoint { $0 <= y + AvatarAligner.epsilon }
        : positions.partitionPoint { $0 < y - AvatarAligner.epsilon }
      return spanStart + y + before[count]
    }
  }

  /// Align `text` to the audio `env`.
  static func align(text: String, language: AvatarLanguage, envelope env: AvatarEnvelope)
    -> AvatarAlignment
  {
    let words = words(text, language: language)
    guard let voiced = AvatarVoiced(envelope: env), !words.isEmpty else { return AvatarAlignment() }
    // A text with more sounds than speech holds in this audio is not this audio's text.
    let sounds = words.reduce(0) { $0 + $1.phonemes.count { $0 != AvatarPhonemes.stress } }
    if Double(sounds) > maxSoundsPerSecond * avatarMax(voiced.end - voiced.start, 1) {
      var out = AvatarAlignment()
      out.span = (voiced.start, voiced.end)
      out.fits = false
      return out
    }
    let silence = voiced.valleys.reduce(0) { $0 + $1.duration }
    let total = avatarMax(voiced.end - voiced.start - silence, env.hop)
    let totalWeight = words.reduce(0) { $0 + $1.weight }
    let valleys = Valleys(spanStart: voiced.start, valleys: voiced.valleys)
    // Where the text puts the end of each word but the last.
    var cumulative = 0.0
    let boundaries: [AvatarBoundary] = words.dropLast().map { w in
      cumulative += w.weight
      return AvatarBoundary(at: total * cumulative / totalWeight, strong: w.pause != nil)
    }
    let pairs = AvatarBoundaryMatch.match(
      boundaries: boundaries, positions: valleys.positions,
      meanWord: total / Double(words.count))
    var knots: [(Double, Double)] = [(0, 0)]
    knots += pairs.map { (boundaries[$0.0].at, valleys.positions[$0.1]) }
    knots.append((total, total))
    let warp = AvatarTimeWarp(knots: knots)
    // The silence each word's end was paired with.
    var paired = [Int?](repeating: nil, count: words.count)
    for (i, j) in pairs { paired[i] = j }

    var phonemes: [AvatarPhoneme] = []
    var starts: [AvatarWordStart] = []
    var wordStart = 0.0
    for (index, word) in words.enumerated() {
      let wordEnd = wordStart + total * word.weight / totalWeight
      var spent = 0.0
      let first = phonemes.count
      for p in word.phonemes {
        let x0 = wordStart + (wordEnd - wordStart) * spent / word.weight
        spent += weight(p)
        let x1 = wordStart + (wordEnd - wordStart) * spent / word.weight
        let t0 = valleys.toClip(spanStart: voiced.start, y: warp.apply(x0), after: true)
        let t1 = avatarMax(
          valleys.toClip(spanStart: voiced.start, y: warp.apply(x1), after: false), t0)
        phonemes.append(AvatarPhoneme(p: p, t0: t0, t1: t1))
      }
      starts.append(
        AvatarWordStart(
          t: phonemes.indices.contains(first) ? phonemes[first].t0 : voiced.start,
          word: word.text,
          prevEndsClause: index > 0 && words[index - 1].pause != nil))
      if let pause = word.pause, AvatarPhonemes.isPause(pause) {
        let (t0, t1): (Double, Double)
        if let j = paired[index] {
          (t0, t1) = (valleys.all[j].start, valleys.all[j].end)
        } else {
          let end = phonemes.last?.t1 ?? voiced.end
          (t0, t1) = (end, end)
        }
        phonemes.append(AvatarPhoneme(p: pause, t0: t0, t1: t1))
      }
      wordStart = wordEnd
    }
    var out = AvatarAlignment()
    out.phonemes = phonemes
    out.span = (voiced.start, voiced.end)
    out.matched = pairs.count
    out.boundaries = boundaries.count
    out.words = starts
    return out
  }
}
