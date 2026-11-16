import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/avatar-mouth.json`, which `soyeht-avatar` and the desktop
/// client pass as well: the morph weights, the mouth's motion, the aligner, the acting layer, the
/// runtime and the warp. Parameters and weights agree within the file's tolerance; the rest agree
/// exactly.
private let file = Result { try ContractVector.load("avatar-mouth") }

private func vector() throws -> JsonValue { try file.get() }

private func tolerance() throws -> Double { try vector().number("tolerance") }

private func check(
  _ group: String, _ run: (JsonValue) throws -> JsonValue
) throws {
  #expect(AvatarVector.mouthGroups.contains(group), "\(group) is not a listed group")
  let file = try vector()
  let limit = try tolerance()
  for item in ContractVector.cases(file, group) {
    let name = try item.text("name")
    let why = AvatarVector.difference(try run(item), item["expected"], tolerance: limit)
    #expect(why == nil, "\(group) / \(name): \(why ?? "")")
  }
}

private func params(_ v: JsonValue) throws -> AvatarParams {
  AvatarParams(
    o: try v.number("o"), w: try v.number("w"), r: try v.number("r"), c: try v.number("c"),
    f: try v.number("f"))
}

@Test func theWeightsOfTheParametersAreTheVectors() throws {
  try check("weights") { item in
    let weights = AvatarMorphWeights(params: try params(item["params"]))
    return .object(["weights": AvatarVector.numbers(weights.all)])
  }
}

private func dynamics(_ item: JsonValue) throws -> JsonValue {
  let tune = AvatarTune()
  let clip = AvatarClip(
    phonemes: try AvatarVector.phonemes(item),
    envelope: try AvatarVector.envelope(item["envelope"]), tune: tune)
  let track: [JsonValue] = clip.track.map {
    .object([
      "p": .string(String($0.p)), "kind": .string($0.kind.rawValue), "t0": .number($0.t0),
      "t1": .number($0.t1), "q": AvatarVector.numbers($0.q.array),
    ])
  }
  var mouth = AvatarMouth(tune: tune)
  let frames = try item.integer("frames")
  let samples: [JsonValue] = (0..<frames).map { i in
    let q = mouth.step(
      dt: 1.0 / AvatarVector.fps, target: clip.target(at: Double(i) / AvatarVector.fps + tune.lead))
    return .object([
      "params": AvatarVector.numbers(q.array),
      "weights": AvatarVector.numbers(mouth.morphWeights.all),
    ])
  }
  return .object(["track": .array(track), "samples": .array(samples)])
}

@Test func theTrackAndTheMotionOfTheMouthAreTheVectors() throws {
  try check("dynamics", dynamics)
}

@Test func theAlignerPlacesTheTextOnTheAudioAsTheVectorsSay() throws {
  try check("aligner") { item in
    let a = AvatarAligner.align(
      text: try item.text("text"), language: try AvatarVector.language(try item.text("language")),
      envelope: try AvatarVector.envelope(item["envelope"]))
    let words: [JsonValue] = a.words.map {
      .object([
        "t": .number($0.t), "word": .string($0.word), "prevEndsClause": .bool($0.prevEndsClause),
      ])
    }
    return .object([
      "span": a.span.map { AvatarVector.numbers([$0.start, $0.end]) } ?? .null,
      "fits": .bool(a.fits), "matched": .integer(Int64(a.matched)),
      "boundaries": .integer(Int64(a.boundaries)),
      "phonemes": AvatarVector.phonemeValues(a.phonemes), "words": .array(words),
    ])
  }
}

@Test func theAlignerFindsSoundsInEveryCaseThatHasAVoiceAndAText() throws {
  for item in ContractVector.cases(try vector(), "aligner") {
    let expected = item["expected"]
    let sounds = try expected.items("phonemes").count
    let name = try item.text("name")
    if try item.text("text").isEmpty || expected["fits"] == .bool(false) {
      #expect(sounds == 0, "\(name)")
      continue
    }
    #expect(sounds > 5, "\(name)")
  }
}

private func actingState(_ item: JsonValue) throws -> AvatarActing {
  AvatarActing(
    tune: AvatarActingTune(), seed: try AvatarVector.seed(item),
    scale: AvatarActing.faceScale(headRadii: try AvatarVector.radii(item)))
}

@Test func theActingLayerLivesAsTheVectorsSay() throws {
  try check("acting") { item in
    var acting = try actingState(item)
    let every = try item.integer("sampleEvery")
    var samples: [JsonValue] = []
    var frame = 0
    for scene in try item.items("scenes") {
      // A scene says whether a voice plays; its level and the jaw it drops are there when it does.
      let voice = try scene.flag("voice")
      let input = AvatarActingInput(
        state: try AvatarVector.state(try scene.text("state")), voice: voice,
        db: voice ? try scene.number("db") : nil, wordsKnown: false,
        jaw: scene["jaw"] == .null ? 0 : try scene.number("jaw"))
      let steps = Int((try scene.number("seconds") * AvatarVector.fps).rounded())
      for _ in 0..<steps {
        let f = acting.step(dt: 1.0 / AvatarVector.fps, input: input)
        frame += 1
        if frame % every == 0 {
          samples.append(
            .object([
              "pose": AvatarVector.numbers(f.pose.values), "gaze": .string(f.eye.gaze.rawValue),
              "lid": .string(f.eye.lid.rawValue), "soft": .bool(f.eye.soft),
            ]))
        }
      }
    }
    return .object(["samples": .array(samples)])
  }
}

@Test func theRuntimePerformsAsTheVectorsSay() throws {
  try check("runtime") { item in
    let tune = AvatarTune()
    let envelope = try AvatarVector.envelope(item["envelope"])
    var clip = AvatarClip(phonemes: try AvatarVector.phonemes(item), envelope: envelope, tune: tune)
    let words = try AvatarVector.wordStarts(item)
    if !words.isEmpty { clip = clip.withWords(text: try item.text("text"), words: words) }
    var runtime = AvatarRuntime(
      tune: tune, acting: AvatarActingTune(), seed: try AvatarVector.seed(item),
      scale: AvatarActing.faceScale(headRadii: try AvatarVector.radii(item)))
    runtime.setClip(clip)
    var frame = 0
    var audioStart: Int?
    var samples: [JsonValue] = []
    for scene in try item.items("script") {
      let state = try AvatarVector.state(try scene.text("state"))
      let steps = Int((try scene.number("seconds") * AvatarVector.fps).rounded())
      for _ in 0..<steps {
        var audio: AvatarAudio?
        if try scene.flag("audio") {
          let start = audioStart ?? frame
          audioStart = start
          let position = Double(frame - start) / AvatarVector.fps
          // The level of the envelope at the position, and -90 dB past its end.
          let i = avatarIndex(position / envelope.hop)
          let db = envelope.db.indices.contains(i) ? envelope.db[i] : -90.0
          audio = AvatarAudio(position: position, db: db)
        }
        let f = runtime.step(dt: 1.0 / AvatarVector.fps, state: state, audio: audio)
        frame += 1
        if frame % 6 == 0 {
          let q = f.mouth
          samples.append(
            .object([
              "mouth": AvatarVector.numbers(q.array), "jaw": .number(f.acting.pose.jaw),
              "brow": .number(f.acting.pose.browL), "state": .string(f.acting.state.rawValue),
            ]))
        }
      }
    }
    return .object(["samples": .array(samples)])
  }
}

/// A pose of a case: the fields it lists, and the rest at rest (`{}` is the rest pose). A field
/// that is listed must be a number.
private func pose(_ v: JsonValue) throws -> AvatarPose {
  let known: Set = [
    "rot", "headX", "headY", "faceX", "faceY", "browL", "browR", "browTilt", "jaw", "cheek",
    "breath", "zoom",
  ]
  guard case .object(let fields) = v, fields.keys.allSatisfy(known.contains) else {
    throw AvatarVector.Unreadable(what: "a pose with known fields")
  }
  let n = { (k: String) throws in fields[k] == nil ? 0 : try v.number(k) }
  return AvatarPose(
    rot: try n("rot"), headX: try n("headX"), headY: try n("headY"), faceX: try n("faceX"),
    faceY: try n("faceY"), browL: try n("browL"), browR: try n("browR"),
    browTilt: try n("browTilt"), jaw: try n("jaw"), cheek: try n("cheek"),
    breath: try n("breath"), zoom: fields["zoom"] == nil ? 1 : try v.number("zoom"))
}

@Test func theWarpReadsEachPointWhereTheVectorsSay() throws {
  try check("warp") { item in
    let warp = try item["warp"].decoded(AvatarWarp.self)
    let pose = try pose(item["pose"])
    let points: [JsonValue] = try item.items("points").map {
      guard case .array(let xy) = $0, xy.count == 2 else {
        throw AvatarVector.Unreadable(what: "a point of two numbers")
      }
      let p = warp.sourcePoint(pose: pose, x: try xy[0].asNumber(), y: try xy[1].asNumber())
      return AvatarVector.numbers([p.x, p.y])
    }
    return .object(["points": .array(points)])
  }
}

@Test func theEyeFrameOfEveryLookIsTheOneTheVectorsName() throws {
  try check("eyeFrames") { item in
    func parse<T>(_ key: String, _ options: [String: T]) throws -> T {
      let id = try item.text(key)
      guard let value = options[id] else { throw AvatarVector.Unreadable(what: "\(key) \(id)") }
      return value
    }
    let eye = AvatarEye(
      gaze: try parse(
        "gaze",
        [
          "center": .center, "left": .left, "right": .right, "up": .up, "down": .down,
          "upleft": .upleft,
        ]),
      lid: try parse("lid", ["open": .open, "half": .half, "closed": .closed]),
      soft: try item.flag("soft"))
    return .object(["frame": AvatarRenderer.patchId(for: eye).map { .string($0) } ?? .null])
  }
}

@Test func everyGroupOfTheVectorFilesIsRunHere() throws {
  for (name, run) in [
    ("avatar-mouth", AvatarVector.mouthGroups), ("avatar-package", AvatarVector.packageGroups),
  ] {
    let present = AvatarVector.groups(of: try ContractVector.load(name))
    let unpinned = present.subtracting(run).subtracting(AvatarVector.notYetAdopted)
    #expect(unpinned.isEmpty, "\(name) has groups no Swift test runs: \(unpinned.sorted())")
    #expect(run.subtracting(present).isEmpty, "\(name) lacks groups the tests run")
  }
}

/// The accent a word gives the acting layer when it is the last of a question, or ends in `!`:
/// two branches of `wordStart` that the vector files do not reach.
@Test func aQuestionAndAnExclamationAccentTheirLastWord() {
  func browAfter(_ call: (inout AvatarActing) -> Void) -> Double {
    var acting = AvatarActing(tune: AvatarActingTune(), seed: 4, scale: 1)
    call(&acting)
    let input = AvatarActingInput(state: .speaking, voice: false, db: nil, wordsKnown: true, jaw: 0)
    var brow = 0.0
    for _ in 0..<12 {
      brow = max(brow, acting.step(dt: 1.0 / 60, input: input).pose.browL)
    }
    return brow
  }
  let plain = browAfter { _ in }
  let question = browAfter {
    $0.wordStart("mundo?", index: 1, count: 2, prevEndsClause: false, text: "Olá mundo?")
  }
  let exclamation = browAfter {
    $0.wordStart("Olá!", index: 0, count: 2, prevEndsClause: false, text: "Olá! Tudo bem")
  }
  // A question that is not at its end, and a short word with an exclamation, do not accent.
  let notLast = browAfter {
    $0.wordStart("mundo?", index: 0, count: 2, prevEndsClause: false, text: "mundo? sim")
  }
  let short = browAfter {
    $0.wordStart("a!", index: 0, count: 1, prevEndsClause: false, text: "a!")
  }
  #expect(question > plain + 0.5)
  #expect(exclamation > plain + 0.5)
  #expect(notLast == plain)
  #expect(short == plain)
}
