import Foundation

/// When a word of the text starts, in clip time.
struct AvatarWordStart: Equatable, Sendable {
  var t: Double
  var word: String
  /// The word before it ended a clause (it was followed by a pause mark).
  var prevEndsClause: Bool
}

/// A clip is one stretch of speech whose phonemes are known before it plays: the target track is
/// built once and read by the audio clock. It may carry the start of each word, so the acting
/// layer can accent the content words.
struct AvatarClip: Sendable {
  private(set) var track: [AvatarTarget]
  private(set) var text = ""
  private(set) var words: [AvatarWordStart] = []

  /// Build the track from timed phonemes, with the voice envelope when there is one (without it
  /// every vowel has the plain gain).
  init(phonemes: [AvatarPhoneme], envelope: AvatarEnvelope?, tune: AvatarTune) {
    if let envelope {
      let reference = envelope.referenceDb
      track = AvatarTrack.build(
        phonemes, level: { envelope.gain(reference: reference, t0: $0, t1: $1) }, tune: tune)
    } else {
      track = AvatarTrack.build(phonemes, level: { _, _ in 1.0 }, tune: tune)
    }
  }

  /// Say which words the clip holds and when each starts, and the whole text, so the runtime
  /// raises the accents itself.
  func withWords(text: String, words: [AvatarWordStart]) -> AvatarClip {
    var clip = self
    clip.text = text
    let finite = words.filter { $0.t.isFinite }
    clip.words = finite.enumerated().sorted { a, b in
      a.element.t != b.element.t ? a.element.t < b.element.t : a.offset < b.offset
    }.map(\.element)
    return clip
  }

  /// The mouth target at the clip time `t` (the position of the audio actually playing).
  func target(at t: Double) -> AvatarParams? { AvatarTrack.target(in: track, at: t) }

  private static let energyFloorDb = -50.0
  private static let energyRangeDb = 30.0

  /// The floor tier of ADR 0166 (the mouth input is energy only): no text and no timings, so the
  /// opening follows the loudness and the lips take a neutral shape. `nil` is silence.
  static func energyTarget(db: Double) -> AvatarParams? {
    let level = AvatarEnvelope.sanitize(db)
    guard level > energyFloorDb else { return nil }
    let open = min(max((level - energyFloorDb) / energyRangeDb, 0), 1)
    return AvatarParams(o: open * 0.8, w: 0.55, r: 0, c: 0, f: 0)
  }
}

/// The mouth dynamics: five parameters that follow their targets with critically damped motion
/// (fast attack, slower release) and a slew limit, and the morph weights that follow the
/// parameters' weights through one more critically damped filter.
///
/// The filter is linear, so the weights keep summing to 1: the mouth passes between shapes by
/// mixing their geometry and never through a third shape, and it never steps. There is no code
/// path here that picks a drawing; the only output is parameters and weights.
struct AvatarMouth: Sendable {
  private let tune: AvatarTune
  private var dims: [AvatarFollow]
  /// The eight viseme weights; the rest weight is what is left, so it needs no follower.
  private var weights: [AvatarFollow]
  /// Seconds since something was last said. Infinite at the start: the lips stay at rest and do
  /// not close before the first word.
  private var silent = Double.infinity
  private(set) var params = AvatarParams.rest

  init(tune: AvatarTune) {
    self.tune = tune
    let rest = AvatarParams.rest.array
    dims = rest.map { AvatarFollow(value: $0, smooth: 0.1) }
    weights = (0..<AvatarMorphWeights.visemeCount).map { _ in
      AvatarFollow(value: 0, smooth: 0.1)
    }
  }

  /// The morph weights after the last step.
  var morphWeights: AvatarMorphWeights {
    AvatarMorphWeights(visemes: weights.map { max($0.value, 0) })
  }

  /// Advance by `dt` seconds towards `target` (`nil` is silence) and return the new parameters.
  /// A step whose `dt` is not a finite number is skipped, and a target with a non-finite
  /// parameter is silence: the state never becomes non-finite.
  @discardableResult
  mutating func step(dt: Double, target: AvatarParams?) -> AvatarParams {
    guard dt.isFinite else { return params }
    let dt = min(max(dt, 1e-4), 0.1)
    let target = target.flatMap { $0.array.allSatisfy(\.isFinite) ? $0 : nil }
    let goal: AvatarParams
    if let target {
      silent = 0
      goal = target
    } else {
      silent += dt
      if silent < tune.silenceClose {
        goal = AvatarParams(o: params.o * 0.4, w: params.w, r: params.r, c: params.c, f: params.f)
      } else if silent < tune.silenceRest {
        goal = AvatarParams(o: 0, w: 0.5, r: 0, c: tune.pauseClosure, f: 0)
      } else {
        goal = .rest
      }
    }
    return advance(dt: dt, goal: goal, relaxing: target == nil && silent >= tune.silenceRest)
  }

  /// Advance by `dt` seconds holding an expression (the smile of a listening avatar): a target of
  /// the mouth that is not speech. It does not restart the silence, so leaving the expression
  /// goes to rest and not through the pressed lips of a pause.
  @discardableResult
  mutating func hold(dt: Double, expression: AvatarParams) -> AvatarParams {
    guard dt.isFinite, expression.array.allSatisfy(\.isFinite) else { return params }
    let dt = min(max(dt, 1e-4), 0.1)
    silent += dt
    return advance(dt: dt, goal: expression, relaxing: false)
  }

  /// Move every parameter towards `goal`; `relaxing` is the slow return to rest after a long
  /// silence.
  private mutating func advance(dt: Double, goal: AvatarParams, relaxing: Bool) -> AvatarParams {
    let rest = AvatarParams.rest.array
    let goal = goal.array
    var out = [Double](repeating: 0, count: AvatarParams.count)
    for k in 0..<AvatarParams.count {
      let opening = abs(goal[k] - rest[k]) > abs(dims[k].value - rest[k])
      let smooth = relaxing ? tune.relax : (opening ? tune.attack[k] : tune.release[k])
      let before = dims[k].value
      dims[k].stepToward(goal[k], smooth: smooth, dt: dt)
      // Slew limit: no jump in one frame.
      let limit = tune.maxRate[k] * dt * (relaxing ? 0.5 : 1.0)
      dims[k].limitStep(before: before, maxDelta: limit, dt: dt)
      out[k] = avatarClamp01(dims[k].value)
    }
    params = AvatarParams(array: out)
    stepWeights(dt: dt, relaxing: relaxing)
    return params
  }

  private mutating func stepWeights(dt: Double, relaxing: Bool) {
    let goal = AvatarMorphWeights(params: params)
    let smooth = relaxing ? tune.relax * 0.6 : tune.weightSmooth
    for index in weights.indices {
      weights[index].stepToward(goal.visemes[index], smooth: smooth, dt: dt)
    }
  }
}
