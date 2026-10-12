import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/loop-panel.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("loop-panel") }

private func stop(_ name: String?) throws -> LoopStopWhen {
  try JsonValue.string(name ?? "").decoded(LoopStopWhen.self)
}

@Test func theLoopPanelSaysWhatTheVectorSays() throws {
  let all = try vector()
  let words = all["words"]
  #expect(LoopPresentation.title == words["title"].string)
  #expect(LoopPresentation.promptHeading == words["promptHeading"].string)
  #expect(LoopPresentation.whenHeading == words["whenHeading"].string)
  #expect(LoopPresentation.everyInterval == words["everyInterval"].string)
  #expect(LoopPresentation.whenIdle == words["whenIdle"].string)
  #expect(LoopPresentation.idleExplanation == words["idleExplanation"].string)
  #expect(LoopPresentation.workerRole == words["workerRole"].string)
  #expect(LoopPresentation.leadRole == words["leadRole"].string)
  #expect(LoopPresentation.cancelTitle == words["cancel"].string)
  #expect(LoopPresentation.applyTitle == words["apply"].string)
  #expect(LoopPresentation.rules == (words["rules"].array ?? []).compactMap(\.string))

  let defaults = all["defaults"]
  let start = LoopPresentation.initial(nil)
  #expect(start.prompt == defaults["prompt"].string)
  #expect(Int(start.interval) == defaults["interval"].int)
  #expect(Int(start.maxRounds) == defaults["maxRounds"].int)
  #expect(start.stopWhen == (try stop(defaults["stopWhen"].string)))
  #expect(start.enabled == (defaults["enabled"].bool == true))

  let intervals = ContractVector.cases(all, "intervals")
  #expect(
    LoopPresentation.intervalChoices.map(Int.init) == intervals.compactMap { $0["seconds"].int })
  for item in intervals {
    #expect(
      LoopPresentation.intervalLabel(UInt32(item["seconds"].int ?? 0)) == item["label"].string)
  }
  let rounds = ContractVector.cases(all, "rounds")
  #expect(LoopPresentation.roundChoices.map(Int.init) == rounds.compactMap { $0["rounds"].int })
  for item in rounds {
    #expect(LoopPresentation.roundLabel(UInt32(item["rounds"].int ?? 0)) == item["label"].string)
  }
  let stops = ContractVector.cases(all, "stops")
  #expect(try LoopPresentation.stopChoices == stops.map { try stop($0["stopWhen"].string) })
  for item in stops {
    let value = try stop(item["stopWhen"].string)
    #expect(LoopPresentation.stopLabel(value) == item["label"].string)
    #expect(LoopPresentation.stopChoiceTitle(value) == item["choice"].string)
  }
  for item in ContractVector.cases(all, "status") {
    #expect(
      LoopPresentation.status(
        enabled: item["enabled"].bool == true, paneName: item["paneName"].string ?? "",
        rounds: UInt32(item["rounds"].int ?? 0)) == item["text"].string)
  }
  for item in ContractVector.cases(all, "initial") {
    let config = try? item["config"].decoded(LoopConfig.self)
    let start = LoopPresentation.initial(config)
    #expect(start.prompt == item["prompt"].string, "\(item["name"].string ?? "")")
    #expect(Int(start.interval) == item["interval"].int)
    #expect(Int(start.maxRounds) == item["maxRounds"].int)
    #expect(start.stopWhen == (try stop(item["stopWhen"].string)))
    #expect(start.enabled == (item["enabled"].bool == true))
  }
  for item in ContractVector.cases(all, "canApply") {
    #expect(
      LoopPresentation.canApply(
        enabled: item["enabled"].bool == true, prompt: item["prompt"].string ?? "")
        == (item["ok"].bool == true))
  }
}

@MainActor @Test func tickAndStopEventsChangeTheLoopTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "events") {
    let client = EngineClient(socket: "/tmp/sn-loop-vector/engine.sock")
    client.paneLoops["p"] = try item["config"].decoded(LoopConfig.self)
    var params = item["params"]
    guard case .object(var fields) = params else { return }
    fields["eventId"] = .string("1")
    fields["at"] = .string("2026-10-11T12:00:00Z")
    params = .object(fields)
    let name = try JsonValue.string(item["event"].string ?? "").decoded(EventName.self)
    try client.applyLoopEvent(name, params)
    let config = try #require(client.paneLoops["p"])
    #expect(Int(config.rounds ?? 0) == item["rounds"].int, "\(item["name"].string ?? "")")
    #expect(config.enabled == (item["enabled"].bool == true))
  }
}
