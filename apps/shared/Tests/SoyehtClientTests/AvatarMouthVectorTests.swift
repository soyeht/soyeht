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

private func tolerance() throws -> Double { try #require(try vector()["tolerance"].double) }

private func check(
  _ group: String, _ run: (JsonValue) throws -> JsonValue
) throws {
  let file = try vector()
  let limit = try tolerance()
  for item in ContractVector.cases(file, group) {
    let name = item["name"].string ?? "?"
    let why = AvatarVector.difference(try run(item), item["expected"], tolerance: limit)
    #expect(why == nil, "\(group) / \(name): \(why ?? "")")
  }
}

private func params(_ v: JsonValue) -> AvatarParams {
  let n = { (k: String) in v[k].double ?? 0 }
  return AvatarParams(o: n("o"), w: n("w"), r: n("r"), c: n("c"), f: n("f"))
}

@Test func theWeightsOfTheParametersAreTheVectors() throws {
  try check("weights") { item in
    let weights = AvatarMorphWeights(params: params(item["params"]))
    return .object(["weights": AvatarVector.numbers(weights.all)])
  }
}

private func dynamics(_ item: JsonValue) -> JsonValue {
  let tune = AvatarTune()
  let clip = AvatarClip(
    phonemes: AvatarVector.phonemes(item["phonemes"]),
    envelope: AvatarVector.envelope(item["envelope"]), tune: tune)
  let track: [JsonValue] = clip.track.map {
    .object([
      "p": .string(String($0.p)), "kind": .string($0.kind.rawValue), "t0": .number($0.t0),
      "t1": .number($0.t1), "q": AvatarVector.numbers($0.q.array),
    ])
  }
  var mouth = AvatarMouth(tune: tune)
  let frames = item["frames"].int ?? 0
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
    guard let envelope = AvatarVector.envelope(item["envelope"]) else { return .null }
    let a = AvatarAligner.align(
      text: item["text"].string ?? "",
      language: AvatarLanguage(id: item["language"].string ?? ""), envelope: envelope)
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
    let sounds = expected["phonemes"].array?.count ?? 0
    if item["text"].string?.isEmpty == true || expected["fits"] == .bool(false) {
      #expect(sounds == 0, "\(item["name"].string ?? "")")
      continue
    }
    #expect(sounds > 5, "\(item["name"].string ?? "")")
  }
}

private func actingState(_ item: JsonValue) -> AvatarActing {
  AvatarActing(
    tune: AvatarActingTune(), seed: AvatarVector.seed(item),
    scale: AvatarActing.faceScale(headRadii: AvatarVector.radii(item)))
}

@Test func theActingLayerLivesAsTheVectorsSay() throws {
  try check("acting") { item in
    var acting = actingState(item)
    let every = item["sampleEvery"].int ?? 30
    var samples: [JsonValue] = []
    var frame = 0
    for scene in item["scenes"].array ?? [] {
      let voice = scene["voice"].bool ?? false
      let input = AvatarActingInput(
        state: AvatarVector.state(scene["state"].string), voice: voice,
        db: voice ? (scene["db"].double ?? -20) : nil, wordsKnown: false,
        jaw: scene["jaw"].double ?? 0)
      let steps = Int(((scene["seconds"].double ?? 0) * AvatarVector.fps).rounded())
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
    let envelope = AvatarVector.envelope(item["envelope"])
    var clip = AvatarClip(
      phonemes: AvatarVector.phonemes(item["phonemes"]), envelope: envelope, tune: tune)
    let words = AvatarVector.wordStarts(item["words"])
    if !words.isEmpty { clip = clip.withWords(text: item["text"].string ?? "", words: words) }
    var runtime = AvatarRuntime(
      tune: tune, acting: AvatarActingTune(), seed: AvatarVector.seed(item),
      scale: AvatarActing.faceScale(headRadii: AvatarVector.radii(item)))
    runtime.setClip(clip)
    var frame = 0
    var audioStart: Int?
    var samples: [JsonValue] = []
    for scene in item["script"].array ?? [] {
      let state = AvatarVector.state(scene["state"].string)
      let steps = Int(((scene["seconds"].double ?? 0) * AvatarVector.fps).rounded())
      for _ in 0..<steps {
        var audio: AvatarAudio?
        if scene["audio"].bool ?? false {
          let start = audioStart ?? frame
          audioStart = start
          let position = Double(frame - start) / AvatarVector.fps
          let db = envelope.map { e in
            let i = avatarIndex(position / e.hop)
            return e.db.indices.contains(i) ? e.db[i] : -90.0
          }
          audio = AvatarAudio(position: position, db: db ?? -90.0)
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

private func pose(_ v: JsonValue) -> AvatarPose {
  let n = { (k: String) in v[k].double ?? 0 }
  return AvatarPose(
    rot: n("rot"), headX: n("headX"), headY: n("headY"), faceX: n("faceX"), faceY: n("faceY"),
    browL: n("browL"), browR: n("browR"), browTilt: n("browTilt"), jaw: n("jaw"),
    cheek: n("cheek"), breath: n("breath"), zoom: v["zoom"] == .null ? 1 : n("zoom"))
}

@Test func theWarpReadsEachPointWhereTheVectorsSay() throws {
  try check("warp") { item in
    let warp = try item["warp"].decoded(AvatarWarp.self)
    let pose = pose(item["pose"])
    let points: [JsonValue] = (item["points"].array ?? []).map {
      let xy = $0.array ?? []
      let p = warp.sourcePoint(
        pose: pose, x: xy.first?.double ?? 0, y: xy.dropFirst().first?.double ?? 0)
      return AvatarVector.numbers([p.x, p.y])
    }
    return .object(["points": .array(points)])
  }
}
