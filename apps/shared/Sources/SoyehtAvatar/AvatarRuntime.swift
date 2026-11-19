import Foundation

/// The audio that plays now.
struct AvatarAudio: Equatable, Sendable {
  /// The position in the clip, in seconds, of the sound coming out of the speakers now.
  var position: Double
  /// Its level in dB, when the host measures it (used to accent speech without word timings, and
  /// as the whole input of the energy-only tier).
  var db: Double?
}

/// What to draw this frame.
struct AvatarFrame: Equatable, Sendable {
  var mouth: AvatarParams
  /// The weights of the nine drawings' lip contours, summing to 1.
  var weights: AvatarMorphWeights
  var acting: AvatarActingFrame
}

/// The whole performance of one avatar: the mouth, the acting layer and the clip that is playing.
/// The host calls `step` once per frame with the position of the audio it is actually playing
/// (the audio clock), and draws what comes back. A port of `soyeht-avatar`'s `runtime`.
struct AvatarRuntime: Sendable {
  /// The chin drops this many reference pixels at the widest opening.
  private static let jawPixels = 4.6

  private let lead: Double
  private let expression: AvatarExpressionTune
  private var mouth: AvatarMouth
  private var acting: AvatarActing
  private var clip: AvatarClip?
  /// How many words of the clip the audio has reached.
  private var wordsReached = 0

  /// A runtime for a face `scale` times the reference face (`AvatarActing.faceScale`).
  init(tune: AvatarTune, acting: AvatarActingTune, seed: UInt64, scale: Double) {
    lead = tune.lead
    expression = acting.expression
    mouth = AvatarMouth(tune: tune)
    self.acting = AvatarActing(tune: acting, seed: seed, scale: scale)
  }

  /// A runtime for a package's avatar: its acting constants and the size of its face.
  init(package: AvatarPackage, tune: AvatarTune, seed: UInt64) {
    self.init(
      tune: tune, acting: package.acting, seed: seed,
      scale: AvatarActing.faceScale(headRadii: package.warp.head.radii))
  }

  /// Use this clip's phonemes for the audio that plays from now on. Without a clip the mouth is
  /// driven by the level alone.
  mutating func setClip(_ clip: AvatarClip?) {
    self.clip = clip
    wordsReached = 0
  }

  /// The mouth target for the audio that plays: the clip's, and when the clip has nothing to say
  /// while the audio is loud (a text the letter rules could not read, a pause the text did not
  /// have) the level's, so a voice never plays over a shut mouth.
  private func speechTarget(_ audio: AvatarAudio) -> AvatarParams? {
    let fromClip = audio.position.isFinite ? clip?.target(at: audio.position + lead) : nil
    return fromClip ?? audio.db.flatMap(AvatarClip.energyTarget)
  }

  /// The resting expression of a state, while nothing is said: a target of the continuous mouth.
  private func expressionTarget(_ state: AvatarState) -> AvatarParams? {
    switch state {
    case .listening: expression.listening
    case .thinking: expression.thinking
    case .idle, .speaking: nil
    }
  }

  /// Raise the accent of the word the audio has just reached (only the latest, when a long frame
  /// crosses several), and follow a seek backwards.
  private mutating func wordAccents(_ audio: AvatarAudio?) {
    guard let clip, let audio, audio.position.isFinite else { return }
    let words = clip.words
    let low = words.partitionPoint { $0.t <= audio.position }
    if low > wordsReached {
      let word = words[low - 1]
      acting.wordStart(
        word.word, index: low - 1, count: words.count, prevEndsClause: word.prevEndsClause,
        text: clip.text)
    }
    wordsReached = low
  }

  /// Advance by `dt` seconds. `state` is the conversation's state; `audio` is `nil` when nothing
  /// is playing. The mouth steps first: the acting layer drops the chin with it.
  mutating func step(dt: Double, state: AvatarState, audio rawAudio: AvatarAudio?) -> AvatarFrame {
    let audio = rawAudio.map {
      AvatarAudio(position: $0.position, db: $0.db.map(AvatarEnvelope.sanitize))
    }
    let params: AvatarParams
    if let audio {
      params = mouth.step(dt: dt, target: speechTarget(audio))
    } else if let held = expressionTarget(state) {
      // An expression is not speech: it does not restart the silence.
      params = mouth.hold(dt: dt, expression: held)
    } else {
      params = mouth.step(dt: dt, target: nil)
    }
    wordAccents(audio)
    // The chin follows the opening, and drops a little less when the lips are pressed.
    let jaw = params.o * Self.jawPixels * (0.8 + 0.2 * (1.0 - params.c))
    let frame = acting.step(
      dt: dt,
      input: AvatarActingInput(
        state: state, voice: audio != nil, db: audio?.db,
        wordsKnown: !(clip?.words.isEmpty ?? true), jaw: jaw))
    return AvatarFrame(mouth: params, weights: mouth.morphWeights, acting: frame)
  }
}
