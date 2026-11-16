import Foundation

/// The numbers of the acting layer: the defaults are the lab's. A package may override any of
/// them; a field it leaves out keeps the default, and a field that is present must be a value of
/// its type (a `null` is not "absent"). Times are seconds, angles are degrees, and distances are
/// pixels of the reference face (`AvatarActing.referenceHead`): the acting layer scales them to
/// the size of the real face.
struct AvatarActingTune: Equatable, Sendable, Decodable {
  var blink = AvatarBlinkTune()
  var gaze = AvatarGazeTune()
  var head = AvatarHeadTune()
  var brow = AvatarBrowTune()
  var breath = AvatarBreathTune()
  var stress = AvatarStressTune()
  var expression = AvatarExpressionTune()

  init() {}

  private enum Keys: String, CodingKey {
    case blink, gaze, head, brow, breath, stress, expression
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    blink = try c.value(.blink, AvatarBlinkTune())
    gaze = try c.value(.gaze, AvatarGazeTune())
    head = try c.value(.head, AvatarHeadTune())
    brow = try c.value(.brow, AvatarBrowTune())
    breath = try c.value(.breath, AvatarBreathTune())
    stress = try c.value(.stress, AvatarStressTune())
    expression = try c.value(.expression, AvatarExpressionTune())
  }
}

extension KeyedDecodingContainer {
  /// The value under `key`, or `fallback` when the key is absent. A key that is present with
  /// `null` or with another type throws, as the Rust reader does.
  fileprivate func value<T: Decodable>(_ key: Key, _ fallback: T) throws -> T {
    contains(key) ? try decode(T.self, forKey: key) : fallback
  }

  /// A pair of numbers: exactly two.
  fileprivate func pair(_ key: Key, _ fallback: [Double]) throws -> [Double] {
    guard contains(key) else { return fallback }
    let values = try decode([Double].self, forKey: key)
    guard values.count == 2 else {
      throw DecodingError.dataCorruptedError(
        forKey: key, in: self, debugDescription: "Expected two numbers")
    }
    return values
  }
}

struct AvatarBlinkTune: Equatable, Sendable, Decodable {
  /// Range of the gap between blinks.
  var gap = [2.2, 5.8]
  var gapListening = [3.2, 7.5]
  var doubleChance = 0.14
  /// Chance that a blink goes with a glance.
  var onGazeShift = 0.35
  /// Half, closed, half: how long each lid drawing is held.
  var seq = [0.042, 0.07, 0.058]

  init() {}

  private enum Keys: String, CodingKey {
    case gap, gapListening, doubleChance, onGazeShift, seq
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    gap = try c.pair(.gap, gap)
    gapListening = try c.pair(.gapListening, gapListening)
    doubleChance = try c.value(.doubleChance, doubleChance)
    onGazeShift = try c.value(.onGazeShift, onGazeShift)
    if c.contains(.seq) {
      seq = try c.decode([Double].self, forKey: .seq)
      guard seq.count == 3 else {
        throw DecodingError.dataCorruptedError(
          forKey: .seq, in: c, debugDescription: "Expected three numbers")
      }
    }
  }
}

struct AvatarGazeTune: Equatable, Sendable, Decodable {
  var idleGap = [1.8, 4.6]
  var holdAway = [0.45, 1.3]
  var speakingAwayChance = 0.45
  var thinkingHold = [1.2, 2.6]

  init() {}

  private enum Keys: String, CodingKey {
    case idleGap, holdAway, speakingAwayChance, thinkingHold
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    idleGap = try c.pair(.idleGap, idleGap)
    holdAway = try c.pair(.holdAway, holdAway)
    speakingAwayChance = try c.value(.speakingAwayChance, speakingAwayChance)
    thinkingHold = try c.pair(.thinkingHold, thinkingHold)
  }
}

struct AvatarHeadTune: Equatable, Sendable, Decodable {
  var swayRot = 0.45
  var swayX = 1.4
  var swayY = 0.85
  var listenTilt = -2.1
  var listenX = 1.6
  var thinkTilt = 1.0
  var thinkY = -1.4
  var nodSpeak = 13.0
  var nodStart = 14.0
  var nodListen = 9.0

  init() {}

  private enum Keys: String, CodingKey {
    case swayRot, swayX, swayY, listenTilt, listenX, thinkTilt, thinkY
    case nodSpeak, nodStart, nodListen
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    swayRot = try c.value(.swayRot, swayRot)
    swayX = try c.value(.swayX, swayX)
    swayY = try c.value(.swayY, swayY)
    listenTilt = try c.value(.listenTilt, listenTilt)
    listenX = try c.value(.listenX, listenX)
    thinkTilt = try c.value(.thinkTilt, thinkTilt)
    thinkY = try c.value(.thinkY, thinkY)
    nodSpeak = try c.value(.nodSpeak, nodSpeak)
    nodStart = try c.value(.nodStart, nodStart)
    nodListen = try c.value(.nodListen, nodListen)
  }
}

struct AvatarBrowTune: Equatable, Sendable, Decodable {
  var stress = 5.0
  var start = 4.8
  var thinking = 4.0
  var listening = 2.0
  var idleFlick = 2.8

  init() {}

  private enum Keys: String, CodingKey { case stress, start, thinking, listening, idleFlick }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    stress = try c.value(.stress, stress)
    start = try c.value(.start, start)
    thinking = try c.value(.thinking, thinking)
    listening = try c.value(.listening, listening)
    idleFlick = try c.value(.idleFlick, idleFlick)
  }
}

struct AvatarBreathTune: Equatable, Sendable, Decodable {
  var period = 4.4
  var periodSpeaking = 3.7
  var amount = 1.0

  init() {}

  private enum Keys: String, CodingKey { case period, periodSpeaking, amount }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    period = try c.value(.period, period)
    periodSpeaking = try c.value(.periodSpeaking, periodSpeaking)
    amount = try c.value(.amount, amount)
  }
}

struct AvatarStressTune: Equatable, Sendable, Decodable {
  /// A voiced level this far over the recent level is an accent (dB).
  var dbOver = 7.0
  var minGap = 0.9
  /// A word of at least this many letters is a content word.
  var contentLen: UInt = 6
  var chance = 0.75
  var wordGap = 0.65

  init() {}

  private enum Keys: String, CodingKey { case dbOver, minGap, contentLen, chance, wordGap }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    dbOver = try c.value(.dbOver, dbOver)
    minGap = try c.value(.minGap, minGap)
    contentLen = try c.value(.contentLen, contentLen)
    chance = try c.value(.chance, chance)
    wordGap = try c.value(.wordGap, wordGap)
  }
}

/// The expression of the resting mouth, as targets of the continuous mouth (not as pictures: a
/// mouth drawing is never swapped in). While nothing is said, a listening avatar holds a slight
/// smile and a thinking one a pressed, slightly pursed "hm"; the mouth morphs there and back like
/// everywhere else.
struct AvatarExpressionTune: Equatable, Sendable, Decodable {
  var listening = AvatarParams(o: 0.05, w: 0.7, r: 0, c: 0, f: 0)
  var thinking = AvatarParams(o: 0, w: 0.35, r: 0.3, c: 0.6, f: 0)

  init() {}

  private enum Keys: String, CodingKey { case listening, thinking }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    listening = try c.value(.listening, listening)
    thinking = try c.value(.thinking, thinking)
  }
}
