import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/voice-turns.json`, which the Windows and Linux app passes too.
/// Every key a case needs is required: a missing or misspelled one fails the test.
private func vector() throws -> JsonValue { try ContractVector.load("voice-turns") }

extension VoiceTurnConfig {
  /// The overrides a case names; a name this client does not know, or a value of the wrong type,
  /// fails.
  fileprivate init(_ values: JsonValue, case name: String) throws {
    self.init()
    if case .null = values { return }
    guard case .object(let fields) = values else {
      throw ContractVectorError(description: "\(name): config")
    }
    for (key, value) in fields {
      func int() throws -> Int { try need(value.int, "\(name).config.\(key)") }
      func double() throws -> Double { try need(value.double, "\(name).config.\(key)") }
      switch key {
      case "frameMs": frameMs = try int()
      case "startThreshold": startThreshold = try double()
      case "endThreshold": endThreshold = try double()
      case "speechStartMs": speechStartMs = try int()
      case "preRollMs": preRollMs = try int()
      case "historyMs": historyMs = try int()
      case "silenceMs": silenceMs = try int()
      case "incompleteSilenceMs": incompleteSilenceMs = try int()
      case "finalizeTimeoutMs": finalizeTimeoutMs = try int()
      case "bargeInMs": bargeInMs = try int()
      case "probeTimeoutMs": probeTimeoutMs = try int()
      case "halfDuplexTailMs": halfDuplexTailMs = try int()
      case "halfDuplex": halfDuplex = try need(value.bool, "\(name).config.halfDuplex")
      case "continuers":
        continuers = try need(value.array, "\(name).continuers").compactMap(\.string)
      default:
        throw ContractVectorError(description: "\(name): the vector sets \(key), which is unknown")
      }
    }
  }
}

extension VoiceTurnAction {
  /// The action as the vector writes it, without its time.
  fileprivate var fields: [String: JsonValue] {
    switch self {
    case .startTurn(let fromMs): ["do": .string("startTurn"), "fromMs": .integer(Int64(fromMs))]
    case .checkEndOfTurn(let id): ["do": .string("checkEndOfTurn"), "id": .integer(Int64(id))]
    case .finalizeTurn: ["do": .string("finalizeTurn")]
    case .discardTurn: ["do": .string("discardTurn")]
    case .submit(let text, let interrupted):
      ["do": .string("submit"), "text": .string(text), "interrupted": .bool(interrupted)]
    case .cancelReply: ["do": .string("cancelReply")]
    case .stopPlayback: ["do": .string("stopPlayback")]
    case .probeInterruption(let fromMs):
      ["do": .string("probeInterruption"), "fromMs": .integer(Int64(fromMs))]
    case .finishProbe: ["do": .string("finishProbe")]
    case .dropProbe: ["do": .string("dropProbe")]
    }
  }
}

@Test func theVectorSaysWhereItsNumbersCome() throws {
  let all = try vector()
  #expect(all["policy"].string == "x-clientPolicy.voice")
  #expect(all["defaults"] == .null)
  let config = VoiceTurnConfig()
  #expect(config.frameMs * VoiceFrames.sampleRate == VoiceFrames.size * 1_000)
  #expect(config.startThreshold > config.endThreshold)
}

@Test func everyTurnCasePlaysAsTheVectorSays() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "cases") { try play(item, in: all) }
}

@Test func everyPhaseMeetsEveryInputAsTheVectorSays() throws {
  let all = try vector()
  let pairs = ContractVector.cases(all, "pairs")
  for item in pairs { try play(item, in: all) }
  var covered: Set<String> = []
  for item in pairs {
    let phase = try need(item["reach"].string, "pair reach")
    let last = try need(item["steps"].array?.last, "pair steps")
    covered.insert("\(phase) \(try need(last["input"].string, "pair input"))")
  }
  let inputs = [
    "start", "stop", "frames", "endOfTurn", "transcript", "sentenceStarted", "replyFinished",
    "interrupt",
  ]
  for phase in VoiceTurnPhase.allCases {
    for input in inputs {
      #expect(covered.contains("\(phase.rawValue) \(input)"), "\(phase) \(input)")
    }
  }
}

private func play(_ item: JsonValue, in all: JsonValue) throws {
  let name = try need(item["name"].string, "case name")
  let config = try VoiceTurnConfig(item["config"], case: name)
  var machine = VoiceTurnMachine(config)
  var steps: [JsonValue] = []
  if let reach = item["reach"].string {
    steps += try need(all["reach"][reach].array, "\(name): reach \(reach)")
  }
  steps += try need(item["steps"].array, "\(name): steps")
  var clock = -1
  for (index, step) in steps.enumerated() {
    let label = "\(name) step \(index)"
    let at = try need(step["at"].int, "\(label) at")
    #expect(at >= clock, "\(label): steps are in time order")
    var produced: [JsonValue] = []
    for (time, input) in try inputs(of: step, at: at, config: config, label: label) {
      clock = time
      for action in machine.handle(input, at: time) {
        var fields = action.fields
        fields["at"] = .integer(Int64(time))
        produced.append(.object(fields))
      }
    }
    let expected = try need(step["actions"].array, "\(label) actions")
    #expect(produced == expected, "\(label)")
    #expect(
      machine.phase.rawValue == (try need(step["phase"].string, "\(label) phase")), "\(label)")
  }
}

/// The inputs a step stands for, each with its time: a run of frames is one input per frame, one
/// every `frameMs` of the case.
private func inputs(of step: JsonValue, at: Int, config: VoiceTurnConfig, label: String) throws
  -> [(Int, VoiceTurnInput)]
{
  let kind = try need(step["input"].string, "\(label) input")
  switch kind {
  case "frames":
    let count = try need(step["count"].int, "\(label) count")
    let probability = try need(step["p"].double, "\(label) p")
    return (0..<count).map { (at + $0 * config.frameMs, .frame(probability: probability)) }
  case "start": return [(at, .start)]
  case "stop": return [(at, .stop)]
  case "interrupt": return [(at, .interrupt)]
  case "replyFinished": return [(at, .replyFinished)]
  case "endOfTurn":
    return [
      (
        at,
        .endOfTurn(
          id: try need(step["id"].int, "\(label) id"),
          complete: try need(step["complete"].bool, "\(label) complete"))
      )
    ]
  case "transcript":
    return [
      (
        at,
        .transcript(
          try need(step["text"].string, "\(label) text"),
          final: try need(step["final"].bool, "\(label) final"))
      )
    ]
  case "sentenceStarted":
    return [(at, .sentenceStarted(try need(step["text"].string, "\(label) text")))]
  default: throw ContractVectorError(description: "\(label): unknown input \(kind)")
  }
}
