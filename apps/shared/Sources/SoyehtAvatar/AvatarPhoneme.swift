import Foundation

/// Phonemes as the mouth sees them: IPA letters with the lab's few extras (`ʤ ʧ` affricates, `W`
/// a dark l that sounds like u, `æ` a reduced final a), a stress mark and the pause marks. A
/// phoneme is a Unicode scalar, as in `soyeht-avatar`.
struct AvatarPhoneme: Equatable, Sendable {
  var p: Unicode.Scalar
  /// Seconds of clip time. A stress mark `ˈ` has `t0 == t1`.
  var t0: Double
  var t1: Double
}

/// What a phoneme does to the lips.
enum AvatarPhonemeKind: String, Sendable {
  /// A vowel: opening, spread and rounding from the table.
  case vowel
  /// A semivowel (`w`, `W`, `j`): a vowel shape, a little less open.
  case semi
  /// A tongue or throat consonant, or a vowel too short to move the lips.
  case transparent
  /// `p b m`: the lips close.
  case bilabial
  /// `f v`: the lower lip meets the upper teeth.
  case labiodental
  /// A pause mark: the lips rest together.
  case pause
}

enum AvatarPhonemes {
  static let stress: Unicode.Scalar = "ˈ"

  /// Vowel targets `[opening, width, rounding]`.
  static func vowel(_ p: Unicode.Scalar) -> [Double]? {
    switch p {
    case "a": [1.0, 0.55, 0.1]
    case "ɑ": [0.95, 0.5, 0.1]
    case "ɐ": [0.7, 0.5, 0.05]
    case "æ": [0.65, 0.55, 0.0]
    case "ə": [0.5, 0.5, 0.0]
    case "ɛ": [0.8, 0.8, 0.0]
    case "e": [0.6, 0.88, 0.0]
    case "i": [0.3, 1.0, 0.0]
    case "ɪ": [0.35, 0.85, 0.0]
    case "y": [0.3, 0.95, 0.0]
    case "ɔ": [0.88, 0.25, 0.7]
    case "o": [0.66, 0.15, 0.85]
    case "u": [0.3, 0.05, 1.0]
    case "ʊ": [0.35, 0.3, 0.6]
    case "w": [0.2, 0.0, 1.0]
    case "W": [0.25, 0.1, 0.85]
    case "j": [0.2, 0.95, 0.0]
    default: nil
    }
  }

  static func isSemi(_ p: Unicode.Scalar) -> Bool { p == "w" || p == "W" || p == "j" }
  static func isBilabial(_ p: Unicode.Scalar) -> Bool { p == "p" || p == "b" || p == "m" }
  static func isLabiodental(_ p: Unicode.Scalar) -> Bool { p == "f" || p == "v" }

  static func isPalatal(_ p: Unicode.Scalar) -> Bool {
    "ʃʒʧʤɲʎ".unicodeScalars.contains(p)
  }

  static func isSibilant(_ p: Unicode.Scalar) -> Bool { p == "s" || p == "z" }

  static func isPause(_ p: Unicode.Scalar) -> Bool {
    ".,!?;:…—-".unicodeScalars.contains(p)
  }

  /// Marks that carry no sound of their own.
  static func isSkipped(_ p: Unicode.Scalar) -> Bool {
    "ˈˌ˜\u{303}ː ".unicodeScalars.contains(p)
  }
}

/// The loudness of a voice per hop, in dB, and the gain it gives a vowel's opening.
struct AvatarEnvelope: Equatable, Sendable {
  /// One value per hop: `10 * log10(mean square + 1e-12)`.
  var db: [Double]
  var hop: Double

  private static let silenceDb = -50.0
  static let floorDb = -120.0
  private static let ceilingDb = 12.0
  private static let hopRange = 1e-3...1.0

  /// A level as the runtime reads it: not a number is the floor, and the ends are the floor and
  /// the ceiling.
  static func sanitize(_ db: Double) -> Double {
    db.isNaN ? floorDb : min(max(db, floorDb), ceilingDb)
  }

  /// Whether this is an envelope a reader can use: a hop in a sane range, and some levels.
  var isUsable: Bool {
    hop.isFinite && Self.hopRange.contains(hop) && !db.isEmpty
  }

  /// The level of frame `i`, read safely.
  func level(_ i: Int) -> Double {
    db.indices.contains(i) ? Self.sanitize(db[i]) : Self.floorDb
  }

  /// The level that counts as "the voice" in this clip: the 90th percentile of the frames that
  /// are not silence.
  var referenceDb: Double {
    let loud = db.indices.map(level).filter { $0 > Self.silenceDb }.sorted()
    let index = avatarIndex((Double(loud.count) * 0.9).rounded(.down))
    return loud.indices.contains(index) ? loud[index] : -20.0
  }

  /// The gain for the window `t0...t1`: about 0.5 for a soft window to 1.2 for a loud one.
  func gain(reference: Double, t0: Double, t1: Double) -> Double {
    guard isUsable, t0.isFinite, t1.isFinite else { return 1.0 }
    let last = db.count - 1
    let i0 = avatarIndex((t0 / hop).rounded(.down))
    let i1 = min(max(avatarIndex((t1 / hop).rounded(.down)), i0), last)
    // A window past the end of the clip has no voice in it.
    let loudest =
      i0 > last ? -100.0 : (i0...i1).map(level).reduce(-100.0, max)
    return min(max(1.0 + (loudest - reference) / 16.0, 0.5), 1.2)
  }
}

/// Every number worth tuning in the mouth dynamics. The defaults are the validated H0 values of
/// the avatar lab; changing one is a change to a mouth the owner approved. Times are seconds;
/// per-parameter arrays are in the order `o, w, r, c, f`.
struct AvatarTune: Equatable, Sendable {
  /// The mouth leads the audio by this much.
  var lead = 0.045
  /// Smooth times towards a more extreme value.
  var attack = [0.07, 0.08, 0.085, 0.035, 0.045]
  /// Smooth times back towards rest.
  var release = [0.1, 0.095, 0.1, 0.06, 0.06]
  /// A closure target lasts at least this long (about three frames at 24 fps).
  var minClosure = 0.11
  /// A closure starts this much earlier: the lips are together when the burst is heard.
  var closureLead = 0.06
  /// A vowel shorter than this is not animated; it takes the shape of its neighbours.
  var minVowel = 0.045
  /// Vowel openings are pushed this far past their plain value.
  var exaggerate = 1.3
  /// Between words the mouth stays at least this open; lips close only for M B P and pauses.
  var floor = 0.2
  /// After this much silence the lips are together.
  var silenceClose = 0.05
  /// After this much silence they relax into the resting smile.
  var silenceRest = 1.0
  /// Closure of the lips in a pause.
  var pauseClosure = 0.9
  /// Per second: no parameter changes faster than this, so the mouth never changes in one frame.
  var maxRate = [9.0, 7.0, 7.0, 13.0, 12.0]
  /// Smooth time of the slow return to the resting smile after a long silence.
  var relax = 0.22
  /// The morph weights follow the parameters' weights with this smooth time.
  var weightSmooth = 0.04
}
