import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/voice-turns.json`, which the Windows and Linux app passes too.
private func vector() throws -> JsonValue { try ContractVector.load("voice-turns") }

extension VoiceTurnConfig {
  /// The overrides a case names; a name this client does not know fails the test.
  init(_ values: JsonValue) {
    self.init()
    guard case .object(let fields) = values else { return }
    for (name, value) in fields {
      switch name {
      case "frameMs": frameMs = value.int ?? frameMs
      case "startThreshold": startThreshold = value.double ?? startThreshold
      case "endThreshold": endThreshold = value.double ?? endThreshold
      case "speechStartMs": speechStartMs = value.int ?? speechStartMs
      case "preRollMs": preRollMs = value.int ?? preRollMs
      case "silenceMs": silenceMs = value.int ?? silenceMs
      case "incompleteSilenceMs": incompleteSilenceMs = value.int ?? incompleteSilenceMs
      case "bargeInMs": bargeInMs = value.int ?? bargeInMs
      case "probeTimeoutMs": probeTimeoutMs = value.int ?? probeTimeoutMs
      case "halfDuplex": halfDuplex = value.bool ?? halfDuplex
      case "continuers": continuers = (value.array ?? []).compactMap(\.string)
      default: Issue.record("The vector sets \(name), which the client does not know")
      }
    }
  }
}

extension VoiceTurnAction {
  /// The action as the vector writes it, without its time.
  var fields: [String: JsonValue] {
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
    case .dropProbe: ["do": .string("dropProbe")]
    }
  }
}

@Test func theDefaultsAreTheVectorsDefaults() throws {
  let defaults = try vector()["defaults"]
  #expect(VoiceTurnConfig(defaults) == VoiceTurnConfig())
  guard case .object(let fields) = defaults else {
    Issue.record("no defaults")
    return
  }
  #expect(fields.count == 11)
}

@Test func everyTurnCasePlaysAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "cases") {
    let name = item["name"].string ?? ""
    var machine = VoiceTurnMachine(VoiceTurnConfig(item["config"]))
    for (index, step) in (item["steps"].array ?? []).enumerated() {
      let at = try #require(step["at"].int, "\(name) step \(index)")
      var produced: [JsonValue] = []
      for (time, input) in try inputs(of: step, at: at) {
        for action in machine.handle(input, at: time) {
          var fields = action.fields
          fields["at"] = .integer(Int64(time))
          produced.append(.object(fields))
        }
      }
      #expect(produced == (step["actions"].array ?? []), "\(name) step \(index)")
      #expect(machine.phase.rawValue == step["phase"].string, "\(name) step \(index)")
    }
  }
}

/// The inputs a step stands for, each with its time: a run of frames is one input per frame.
private func inputs(of step: JsonValue, at: Int) throws -> [(Int, VoiceTurnInput)] {
  let kind = try #require(step["input"].string)
  switch kind {
  case "frames":
    let count = try #require(step["count"].int)
    let probability = try #require(step["p"].double)
    return (0..<count).map {
      (at + $0 * VoiceTurnConfig().frameMs, .frame(probability: probability))
    }
  case "start": return [(at, .start)]
  case "stop": return [(at, .stop)]
  case "interrupt": return [(at, .interrupt)]
  case "replyFinished": return [(at, .replyFinished)]
  case "endOfTurn":
    let id = try #require(step["id"].int)
    return [(at, .endOfTurn(id: id, complete: step["complete"].bool == true))]
  case "transcript":
    let text = try #require(step["text"].string)
    return [(at, .transcript(text, final: step["final"].bool == true))]
  case "sentenceStarted":
    return [(at, .sentenceStarted(try #require(step["text"].string)))]
  default:
    Issue.record("Unknown input \(kind)")
    return []
  }
}
