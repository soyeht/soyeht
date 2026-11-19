import Foundation

/// The range of every constant of the acting layer a package can set. The acting layer runs on
/// whatever a package says, and a stranger's package must not be able to make the head leave the
/// picture or a number leave the real line: a value of `1e308` makes the pose not a number, and
/// `5000` moves the head 7000 pixels. Distances are in reference pixels, times in seconds, angles
/// in degrees. The same table as `soyeht-avatar`'s `acting_bounds`.
enum AvatarActingBounds {
  struct Bound: Sendable {
    let pointer: String
    let least: Double
    let most: Double
    let value: @Sendable (AvatarActingTune) -> Double
  }

  private static func bound(
    _ pointer: String, _ least: Double, _ most: Double,
    _ value: @escaping @Sendable (AvatarActingTune) -> Double
  ) -> Bound {
    Bound(pointer: pointer, least: least, most: most, value: value)
  }

  private static let gapRange = (0.2, 60.0)
  private static let lidRange = (0.01, 1.0)

  /// Every number of an `AvatarActingTune`, in the order of the document.
  static let table: [Bound] =
    [
      bound("/blink/gap/0", gapRange.0, gapRange.1) { $0.blink.gap[0] },
      bound("/blink/gap/1", gapRange.0, gapRange.1) { $0.blink.gap[1] },
      bound("/blink/gapListening/0", gapRange.0, gapRange.1) { $0.blink.gapListening[0] },
      bound("/blink/gapListening/1", gapRange.0, gapRange.1) { $0.blink.gapListening[1] },
      bound("/blink/doubleChance", 0, 1) { $0.blink.doubleChance },
      bound("/blink/onGazeShift", 0, 1) { $0.blink.onGazeShift },
      bound("/blink/seq/0", lidRange.0, lidRange.1) { $0.blink.seq[0] },
      bound("/blink/seq/1", lidRange.0, lidRange.1) { $0.blink.seq[1] },
      bound("/blink/seq/2", lidRange.0, lidRange.1) { $0.blink.seq[2] },
      bound("/gaze/idleGap/0", gapRange.0, gapRange.1) { $0.gaze.idleGap[0] },
      bound("/gaze/idleGap/1", gapRange.0, gapRange.1) { $0.gaze.idleGap[1] },
      bound("/gaze/holdAway/0", 0.05, 10) { $0.gaze.holdAway[0] },
      bound("/gaze/holdAway/1", 0.05, 10) { $0.gaze.holdAway[1] },
      bound("/gaze/speakingAwayChance", 0, 1) { $0.gaze.speakingAwayChance },
      bound("/gaze/thinkingHold/0", 0.2, 60) { $0.gaze.thinkingHold[0] },
      bound("/gaze/thinkingHold/1", 0.2, 60) { $0.gaze.thinkingHold[1] },
      bound("/head/swayRot", 0, 5) { $0.head.swayRot },
      bound("/head/swayX", 0, 50) { $0.head.swayX },
      bound("/head/swayY", 0, 50) { $0.head.swayY },
      bound("/head/listenTilt", -15, 15) { $0.head.listenTilt },
      bound("/head/listenX", -50, 50) { $0.head.listenX },
      bound("/head/thinkTilt", -15, 15) { $0.head.thinkTilt },
      bound("/head/thinkY", -50, 50) { $0.head.thinkY },
      bound("/head/nodSpeak", 0, 40) { $0.head.nodSpeak },
      bound("/head/nodStart", 0, 40) { $0.head.nodStart },
      bound("/head/nodListen", 0, 40) { $0.head.nodListen },
      bound("/brow/stress", 0, 30) { $0.brow.stress },
      bound("/brow/start", 0, 30) { $0.brow.start },
      bound("/brow/thinking", 0, 30) { $0.brow.thinking },
      bound("/brow/listening", 0, 30) { $0.brow.listening },
      bound("/brow/idleFlick", 0, 30) { $0.brow.idleFlick },
      bound("/breath/period", 1, 30) { $0.breath.period },
      bound("/breath/periodSpeaking", 1, 30) { $0.breath.periodSpeaking },
      bound("/breath/amount", 0, 2) { $0.breath.amount },
      bound("/stress/dbOver", 1, 40) { $0.stress.dbOver },
      bound("/stress/minGap", 0.2, 30) { $0.stress.minGap },
      bound("/stress/contentLen", 1, 64) { Double($0.stress.contentLen) },
      bound("/stress/chance", 0, 1) { $0.stress.chance },
      bound("/stress/wordGap", 0, 30) { $0.stress.wordGap },
    ] + expression("listening") { $0.expression.listening }
    + expression("thinking") { $0.expression.thinking }

  /// The resting expressions are targets of the mouth: parameters in 0 to 1.
  private static func expression(
    _ name: String, _ params: @escaping @Sendable (AvatarActingTune) -> AvatarParams
  ) -> [Bound] {
    [
      bound("/expression/\(name)/o", 0, 1) { params($0).o },
      bound("/expression/\(name)/w", 0, 1) { params($0).w },
      bound("/expression/\(name)/r", 0, 1) { params($0).r },
      bound("/expression/\(name)/c", 0, 1) { params($0).c },
      bound("/expression/\(name)/f", 0, 1) { params($0).f },
    ]
  }

  /// The ranges whose first number may not pass the second.
  private static let ordered: [(String, @Sendable (AvatarActingTune) -> [Double])] = [
    ("/blink/gap", { $0.blink.gap }),
    ("/blink/gapListening", { $0.blink.gapListening }),
    ("/gaze/idleGap", { $0.gaze.idleGap }),
    ("/gaze/holdAway", { $0.gaze.holdAway }),
    ("/gaze/thinkingHold", { $0.gaze.thinkingHold }),
  ]

  /// The pointer of the first constant out of its range, or `nil`.
  static func firstOutOfRange(_ tune: AvatarActingTune) -> String? {
    for bound in table {
      let v = bound.value(tune)
      if !(v.isFinite && bound.least <= v && v <= bound.most) { return bound.pointer }
    }
    return ordered.first { $0.1(tune)[0] > $0.1(tune)[1] }?.0
  }
}
