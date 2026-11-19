import Foundation

/// The four states of the avatar, as the acting layer sees them.
public enum AvatarState: String, Sendable {
  case idle, listening, thinking, speaking
}

/// Where the eyes look: the choice of the eye drawing.
enum AvatarGaze: String, Sendable {
  case center, left, right, up, down, upleft

  /// The unit direction the head follows a little.
  fileprivate var direction: (Double, Double) {
    switch self {
    case .left: (-1, 0)
    case .right: (1, 0)
    case .upleft: (-0.8, -1)
    case .up: (0, -1)
    case .down: (0, 1)
    case .center: (0, 0)
    }
  }
}

/// The lid drawing: the base picture's open eyes, the half-closed frame, or the blink frame.
enum AvatarLid: String, Sendable {
  case open, half, closed
}

struct AvatarEye: Equatable, Sendable {
  var gaze: AvatarGaze
  var lid: AvatarLid
  /// The soft eyes of listening.
  var soft: Bool
}

/// What the host tells the layer each frame.
struct AvatarActingInput: Sendable {
  /// The state the conversation is in (speaking follows the voice itself).
  var state: AvatarState
  /// Audio is playing now.
  var voice: Bool
  /// The level of the audio now, in dB, when it plays.
  var db: Double?
  /// The words are known (from the TTS timings), so the accents come from the words.
  var wordsKnown: Bool
  var jaw: Double
}

/// The pose the warp draws. Lengths (`head*`, `face*`, `brow*`, `jaw`, `cheek`) are pixels of the
/// avatar's canvas; `rot` is degrees; `breath` is 0 to 1.4 and `zoom` about 1, both unitless.
struct AvatarPose: Equatable, Sendable {
  var rot = 0.0
  var headX = 0.0
  var headY = 0.0
  var faceX = 0.0
  var faceY = 0.0
  var browL = 0.0
  var browR = 0.0
  var browTilt = 0.0
  var jaw = 0.0
  var cheek = 0.0
  var breath = 0.0
  var zoom = 1.0

  /// The twelve numbers in the order of the vector files.
  var values: [Double] {
    [rot, headX, headY, faceX, faceY, browL, browR, browTilt, jaw, cheek, breath, zoom]
  }
}

struct AvatarActingFrame: Equatable, Sendable {
  var pose: AvatarPose
  var eye: AvatarEye
  var state: AvatarState
}

/// The acting layer: the four states of the avatar and the small, eased motions that make a still
/// picture feel alive: blinks with in-betweens, saccades, brows, nods, tilts and breathing. It
/// returns the pose of the head and the choice of eye drawing for the frame; the mouth is
/// `AvatarMouth`. A port of `soyeht-avatar`'s `acting`, in seconds, with a seeded generator so the
/// same seed gives the same life in every implementation.
struct AvatarActing: Sendable {
  /// The radii of the head (horizontal, vertical) of the picture the acting constants are
  /// written for, in pixels: the lab's face, in a picture 1024 pixels tall.
  static let referenceHead = [370.0, 390.0]

  /// How much larger than the reference face a face with these head radii is: the geometric mean
  /// of the ratios of the radii. A radius that is not a positive number is the reference.
  static func faceScale(headRadii: [Double]) -> Double {
    let ratio =
      (headRadii[0] / referenceHead[0]) * (headRadii[1] / referenceHead[1])
    return ratio.isFinite && ratio > 0 ? min(max(ratio.squareRoot(), 0.01), 16) : 1
  }

  /// The most the host may ask the jaw to drop, in reference pixels.
  private static let maxJaw = 20.0

  /// A thing that happens a moment later.
  private enum Action {
    case followGaze(AvatarGaze)
    case centerFrom(AvatarGaze)
    case browsBack
    case blink
    case nod(Double)
    case headY(Double)
    case think
  }

  private struct Followers {
    var rot = AvatarFollow(value: 0, smooth: 0.35)
    var hx = AvatarFollow(value: 0, smooth: 0.4)
    var hy = AvatarFollow(value: 0, smooth: 0.4)
    var fx = AvatarFollow(value: 0, smooth: 0.55)
    var fy = AvatarFollow(value: 0, smooth: 0.55)
    var browL = AvatarFollow(value: 0, smooth: 0.14)
    var browR = AvatarFollow(value: 0, smooth: 0.14)
    var tilt = AvatarFollow(value: 0, smooth: 0.16)
    var jaw = AvatarFollow(value: 0, smooth: 0.045)
    var cheek = AvatarFollow(value: 0, smooth: 0.3)
    var nx = AvatarFollow(value: 0, smooth: 0.3)
    var ny = AvatarFollow(value: 0, smooth: 0.3)
  }

  private let tune: AvatarActingTune
  /// The size of the face over the reference face.
  private let scale: Double
  private var rng: AvatarRng
  private var f = Followers()
  private var nod = AvatarKick(frequency: 3.4, damping: 0.45)
  private var nodRot = AvatarKick(frequency: 3.0, damping: 0.5)
  private var chest = AvatarKick(frequency: 2.0, damping: 0.6)
  private var clock = 0.0
  private(set) var state = AvatarState.idle
  private var timers: [(Double, Action)] = []
  private var eye = AvatarEye(gaze: .center, lid: .open, soft: false)
  private var gazeAt: Double
  private var blink: (index: Int, time: Double)?
  private var blinkAt: Double
  private var doubleLeft = false
  private var away = AvatarGaze.upleft
  private var breathPhase: Double
  private var voiceActive = false
  private var voiceLast = -1e9
  private var voiceStart = -1e9
  private var silenceBefore = 0.0
  private var dbEma = -60.0
  private var stressLast = -1e9
  private var browFlickAt: Double
  private var listenNodAt: Double
  private var swayT: Double

  /// An acting layer for a face `scale` times the reference face (`faceScale`).
  init(tune: AvatarActingTune, seed: UInt64, scale: Double) {
    self.tune = tune
    self.scale = scale.isFinite && scale > 0 ? min(max(scale, 0.01), 16) : 1
    var rng = AvatarRng(seed: seed)
    gazeAt = rng.range(of: tune.gaze.idleGap)
    blinkAt = rng.range(of: tune.blink.gap)
    breathPhase = rng.unit()
    browFlickAt = rng.range(5.0, 9.0)
    listenNodAt = rng.range(2.5, 5.0)
    swayT = rng.unit() * 100.0
    self.rng = rng
  }

  private mutating func after(_ seconds: Double, _ action: Action) {
    timers.append((clock + seconds, action))
  }

  private var baseBrow: Double {
    switch state {
    case .thinking: tune.brow.thinking
    case .listening: tune.brow.listening
    default: 0
    }
  }

  private mutating func doBlink(double: Bool) {
    if blink == nil {
      blink = (0, 0)
      doubleLeft = double
    }
  }

  private mutating func look(_ dir: AvatarGaze, hold: Double) {
    if eye.gaze == dir { return }
    eye.gaze = dir
    // The eyes lead; the head follows a little later and comes back with the eyes.
    after(0.09, .followGaze(dir))
    if dir == .center {
      f.nx.target = 0
      f.ny.target = 0
    }
    if dir != .center && rng.unit() < tune.blink.onGazeShift {
      let delay = rng.range(0.02, 0.09)
      after(delay, .blink)
    }
    if hold > 0 { after(hold, .centerFrom(dir)) }
  }

  private mutating func raiseBrows(_ px: Double, hold: Double, asym: Double) {
    f.browL.target = px * (1.0 + asym)
    f.browR.target = px * (1.0 - asym)
    f.tilt.target = px * 0.35
    after(hold, .browsBack)
  }

  private mutating func run(_ action: Action) {
    switch action {
    case .followGaze(let dir):
      if eye.gaze == dir {
        let (dx, dy) = dir.direction
        f.nx.target = dx
        f.ny.target = dy
      }
    case .centerFrom(let dir):
      if eye.gaze == dir { look(.center, hold: 0) }
    case .browsBack:
      let base = baseBrow
      f.browL.target = base
      f.browR.target = base
      f.tilt.target = state == .thinking ? 1.6 : 0
    case .blink:
      doBlink(double: false)
    case .nod(let v):
      nod.kick(v)
    case .headY(let dy):
      f.hy.target += dy
    case .think:
      if state == .thinking {
        look(away, hold: 0)
        raiseBrows(tune.brow.thinking, hold: 0.7, asym: 0.25)
      }
    }
  }

  private mutating func setState(_ next: AvatarState) {
    if next == state { return }
    let prev = state
    state = next
    let head = tune.head
    switch next {
    case .listening: f.cheek.target = 1.3
    case .idle: f.cheek.target = 0.5
    default: f.cheek.target = 0
    }
    eye.soft = next == .listening
    let pose: (Double, Double, Double, Double, Double) =
      switch next {
      case .listening: (head.listenTilt, head.listenX, 0.6, head.listenX * 1.4, 0)
      case .thinking: (head.thinkTilt, -1, head.thinkY, -2.2, -1.8)
      default: (0, 0, 0, 0, 0)
      }
    f.rot.target = pose.0
    f.hx.target = pose.1
    f.hy.target = pose.2
    f.fx.target = pose.3
    f.fy.target = pose.4
    switch next {
    case .listening:
      raiseBrows(tune.brow.listening + 1.5, hold: 0.5, asym: 0)
      look(.center, hold: 0)
    case .thinking:
      away = rng.pick([.upleft, .upleft, .up])
      after(0.12, .think)
      after(0.06, .blink)
      gazeAt = rng.range(of: tune.gaze.thinkingHold)
    default:
      if eye.gaze != .center { look(.center, hold: 0) }
      if prev == .thinking { after(0.04, .blink) }
      if prev == .listening && next != .thinking { nod.kick(80) }
    }
  }

  private mutating func sentenceStart() {
    // Anticipation (a small lift), then the nod, a quick inhale, brows up; sometimes a glance
    // away first.
    f.hy.target -= 1.2
    after(0.095, .headY(1.2))
    nod.kick(-60)
    after(0.11, .nod(tune.head.nodStart * 11.0))
    chest.kick(9)
    let asym = rng.range(-0.25, 0.25)
    raiseBrows(tune.brow.start, hold: 0.42, asym: asym)
    if silenceBefore > 0.9 && rng.unit() < tune.gaze.speakingAwayChance {
      let dir: AvatarGaze = rng.pickWeighted([
        (.up, 3.0), (.left, 2.0), (.right, 2.0), (.down, 1.0),
      ])
      let hold = rng.range(0.35, 0.75)
      look(dir, hold: hold)
    }
  }

  /// An accent of the given strength (1 is a plain one): a nod, a tilt, brows up.
  mutating func stress(_ strength: Double) {
    nod.kick(tune.head.nodSpeak * 9.0 * strength)
    let tilt = rng.range(-14.0, 14.0)
    nodRot.kick(tilt)
    let asym = rng.range(-0.2, 0.2)
    raiseBrows(tune.brow.stress * strength, hold: 0.38, asym: asym)
    if blink == nil {
      // Never blink in the middle of an accent.
      blinkAt = avatarMax(blinkAt, 0.4)
    }
  }

  /// A word starts (from the TTS word timings): accent the content words, and lift the brows at
  /// the end of a question. `prevEndsClause` says the word before ended a clause.
  mutating func wordStart(
    _ word: String, index: Int, count: Int, prevEndsClause: Bool, text: String
  ) {
    let letters = word.unicodeScalars.count { $0.properties.isAlphabetic }
    let last = index + 1 == count
    if last && AvatarText.endsWith("?", in: text) {
      raiseBrows(tune.brow.stress + 2.0, hold: 0.9, asym: 0.15)
      nodRot.kick(-9)
      nod.kick(-8)
      return
    }
    if word.unicodeScalars.last == "!" && letters >= 2 {
      stress(1.15)
      return
    }
    let content = letters >= Int(tune.stress.contentLen) || (prevEndsClause && letters >= 4)
    if content && clock - stressLast > tune.stress.wordGap && rng.unit() < tune.stress.chance {
      stressLast = clock
      let strength = 0.8 + rng.unit() * 0.5
      stress(strength)
    }
  }

  /// Advance by `dt` seconds. A host can hand over anything: a step that is not a finite number
  /// is a step of no time, a level that is not a number or is `-inf` (a silent buffer) is the
  /// floor, and a jaw that is not a number is none.
  mutating func step(dt rawDt: Double, input io: AvatarActingInput) -> AvatarActingFrame {
    let dt = rawDt.isFinite ? min(max(rawDt, 0), 0.1) : 0
    let db = io.db.map(AvatarEnvelope.sanitize)
    let jaw = io.jaw.isFinite ? min(max(io.jaw, 0), Self.maxJaw) : 0
    clock += dt
    swayT += dt
    if io.voice {
      if !voiceActive {
        silenceBefore = clock - voiceLast
        voiceStart = clock
        if silenceBefore > 0.45 { sentenceStart() }
      }
      voiceLast = clock
    }
    voiceActive = io.voice
    let speaking = io.state == .speaking || io.voice || clock - voiceLast < 0.5
    setState(speaking ? .speaking : io.state)

    // Stress is a voiced level clearly above the recent level.
    if io.voice, let db {
      let over = db - dbEma
      if !io.wordsKnown && over > tune.stress.dbOver && clock - stressLast > tune.stress.minGap
        && clock - voiceStart > 0.12
      {
        stressLast = clock
        stress(avatarMin(0.7 + (over - tune.stress.dbOver) / 8.0, 1.4))
      }
      dbEma += (db - dbEma) * avatarMin(dt / 0.8, 1.0)
    }

    for i in timers.indices.reversed() where timers[i].0 <= clock {
      let (_, action) = timers.remove(at: i)
      run(action)
    }
    saccade(dt: dt, voiced: io.voice)
    blinkStep(dt: dt)
    smallLife(dt: dt)
    return pose(dt: dt, speaking: speaking, jaw: jaw)
  }

  /// Saccades: dart, hold, come back.
  private mutating func saccade(dt: Double, voiced: Bool) {
    gazeAt -= dt
    if gazeAt > 0 { return }
    switch state {
    case .idle:
      let dir: AvatarGaze = rng.pickWeighted([
        (.left, 3.0), (.right, 3.0), (.up, 2.0), (.down, 1.5),
      ])
      let hold = rng.range(of: tune.gaze.holdAway)
      look(dir, hold: hold)
      gazeAt = rng.range(of: tune.gaze.idleGap) + 0.7
    case .listening:
      if rng.unit() < 0.5 {
        let dir: AvatarGaze = rng.pickWeighted([(.down, 3.0), (.left, 1.0), (.right, 1.0)])
        let hold = rng.range(0.35, 0.8)
        look(dir, hold: hold)
      }
      gazeAt = rng.range(2.5, 5.5)
    case .thinking:
      let dir: AvatarGaze = rng.pickWeighted([(.upleft, 3.0), (.up, 2.0), (.left, 1.5)])
      look(dir, hold: 0)
      gazeAt = rng.range(of: tune.gaze.thinkingHold)
    case .speaking:
      if !voiced && rng.unit() < 0.3 {
        let dir: AvatarGaze = rng.pick([.up, .up, .left, .left, .right, .right])
        let hold = rng.range(0.3, 0.6)
        look(dir, hold: hold)
      }
      gazeAt = rng.range(1.4, 3.5)
    }
  }

  /// Blinks: three drawings (half, closed, half), never during an accent.
  private mutating func blinkStep(dt: Double) {
    blinkAt -= dt
    if blinkAt <= 0 && blink == nil {
      let double = rng.unit() < tune.blink.doubleChance
      doBlink(double: double)
      let gap = state == .listening ? tune.blink.gapListening : tune.blink.gap
      blinkAt = rng.range(of: gap)
    }
    guard let current = blink else { return }
    var (i, t) = (current.index, current.time)
    let seq = tune.blink.seq
    t += dt
    if t >= seq[i] {
      t = 0
      i += 1
    }
    if i >= seq.count {
      blink = nil
      eye.lid = .open
      if doubleLeft {
        doubleLeft = false
        after(0.12, .blink)
      }
    } else {
      blink = (i, t)
      eye.lid = i == 1 ? .closed : .half
    }
  }

  /// Small life: brow flicks when idle, nods while listening.
  private mutating func smallLife(dt: Double) {
    browFlickAt -= dt
    if browFlickAt <= 0 {
      if state == .idle { raiseBrows(tune.brow.idleFlick, hold: 0.26, asym: 0) }
      browFlickAt = rng.range(5.0, 10.0)
    }
    listenNodAt -= dt
    if listenNodAt <= 0 {
      if state == .listening {
        nod.kick(tune.head.nodListen * 9.0)
        if rng.unit() < 0.5 { after(0.26, .nod(tune.head.nodListen * 6.0)) }
      }
      listenNodAt = rng.range(2.6, 5.5)
    }
  }

  /// Breathing (inhale faster than exhale) and the sway on periods that never line up.
  private mutating func pose(dt: Double, speaking: Bool, jaw: Double) -> AvatarActingFrame {
    let period = speaking ? tune.breath.periodSpeaking : tune.breath.period
    breathPhase = (breathPhase + dt / period).truncatingRemainder(dividingBy: 1.0)
    let br =
      breathPhase < 0.4
      ? avatarEase(breathPhase / 0.4) : 1.0 - avatarEase((breathPhase - 0.4) / 0.6)
    let k = speaking ? 1.4 : (state == .listening ? 0.6 : 1.0)
    let tau = 2.0 * Double.pi
    let h = tune.head
    let t = swayT
    let swayRot =
      k * h.swayRot * (sin(t * tau / 7.3) * 0.7 + sin(t * tau / 3.1 + 1.0) * 0.3)
    let swayX = k * h.swayX * sin(t * tau / 8.9 + 2.0)
    let swayY = k * h.swayY * sin(t * tau / 5.3) - br * 0.9
    f.jaw.target = jaw
    let nodv = nod.step(dt)
    let nodr = nodRot.step(dt)
    let ch = chest.step(dt)
    let (rot, hx, hy) = (f.rot.step(dt), f.hx.step(dt), f.hy.step(dt))
    let (fx, fy) = (f.fx.step(dt), f.fy.step(dt))
    let (browL, browR, tilt) = (f.browL.step(dt), f.browR.step(dt), f.tilt.step(dt))
    let (jawNow, cheek) = (f.jaw.step(dt), f.cheek.step(dt))
    let (nx, ny) = (f.nx.step(dt), f.ny.step(dt))
    let px = scale
    let pose = AvatarPose(
      rot: rot + swayRot + nodr * 0.12 + nodv * 0.05 + nx * 0.35,
      headX: (hx + swayX + nx * 2.2) * px,
      headY: (hy + swayY + nodv * 0.9 + ny * 1.2) * px,
      faceX: (fx + swayX * 0.8 + nx * 3.2) * px,
      faceY: (fy + nodv * 0.7 - br * 0.4 + ny * 1.8) * px,
      browL: browL * px,
      browR: browR * px,
      browTilt: tilt * px,
      jaw: jawNow * px,
      cheek: cheek * px,
      breath: min(max(br * tune.breath.amount + ch * 0.05, 0), 1.4),
      zoom: 1.0 + 0.003 * br + 0.0015 * sin(t * tau / 11.0))
    return AvatarActingFrame(pose: pose, eye: eye, state: state)
  }
}

enum AvatarText {
  /// Whether the text, without the white space at its end (Unicode White_Space, as Rust's
  /// `trim_end`), ends with the scalar.
  static func endsWith(_ scalar: Unicode.Scalar, in text: String) -> Bool {
    text.unicodeScalars.last { !$0.properties.isWhitespace } == scalar
  }
}
