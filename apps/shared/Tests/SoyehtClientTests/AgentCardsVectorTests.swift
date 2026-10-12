import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/agent-cards.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("agent-cards") }

private func date(_ value: String?) throws -> Date {
  try #require(value.flatMap(UsageTimestamp.date(from:)))
}

@Test func elapsedTimeReadsAsMinutesAndSecondsThenHoursAndMinutes() throws {
  for item in ContractVector.cases(try vector(), "elapsedText") {
    let seconds = try #require(item["seconds"].int)
    #expect(AgentElapsedText.text(seconds: seconds) == item["text"].string)
  }
}

@Test func theSilenceNoticeSaysWhatTheVectorSays() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "silence") {
    let name = item["name"].string ?? ""
    let state = try item["paneState"].string.map {
      try JsonValue.string($0).decoded(PaneState.self)
    }
    let presentation = AgentSilencePresentation(
      silentSince: item["silentSince"].string, paneState: state, now: try date(item["now"].string))
    #expect(presentation.elapsedSeconds == item["elapsedSeconds"].int, "\(name)")
    #expect(presentation.escalated == item["escalated"].bool, "\(name)")
    #expect(presentation.elapsed == item["elapsed"].string, "\(name)")
    #expect(presentation.title == item["title"].string, "\(name)")
    #expect(presentation.detail == item["detail"].string, "\(name)")
  }
  #expect(AgentSilencePresentation.cancelTurnTitle == all["silenceLabels"]["cancelTurn"].string)
}

@Test func theQueueCountsAndDescribesWhatWaits() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "queue") {
    let name = item["name"].string ?? ""
    let items = try item["items"].decoded([PromptItem].self)
    let paused = item["paused"].bool == true
    let queue = AgentQueuePresentation(items: items, paused: paused)
    #expect(queue.summary == item["summary"].string, "\(name)")
    #expect(AgentQueuePresentation.isShown(items: items, paused: paused), "\(name)")
    let rows = item["rows"].array ?? []
    #expect(queue.rows.count == rows.count, "\(name)")
    for (row, expected) in zip(queue.rows, rows) {
      #expect(row.sender == expected["sender"].string, "\(name)")
      #expect(row.text == expected["text"].string, "\(name)")
      #expect(row.attachmentCount == expected["attachmentCount"].int, "\(name)")
    }
  }
  #expect(!AgentQueuePresentation.isShown(items: [], paused: false))
  let words = all["queueLabels"]
  #expect(AgentQueuePresentation.resumeTitle == words["resume"].string)
  #expect(AgentQueuePresentation.removeTitle == words["remove"].string)
  #expect(AgentQueuePresentation.sendNowTitle == words["sendNow"].string)
  #expect(AgentQueuePresentation.sendNowHint == words["sendNowHint"].string)
}

@Test func theSignInCardFollowsTheEnginesState() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "signIn") {
    let name = item["name"].string ?? ""
    let methods = try item["authMethods"].decoded([AuthMethod].self)
    let session = try JsonValue.object([
      "harness": .string("claude"), "version": .string("1"), "fingerprint": .string("f"),
      "options": .array([]), "authMethods": .array([]), "queue": .array([]),
      "draft": .bool(false), "role": .string("worker"), "login": item["login"],
    ]).decoded(AgentSession.self)
    let state = AgentSignInState(
      agent: session, authMethods: methods, failure: item["failure"].string)
    #expect(state.title == item["title"].string, "\(name)")
    #expect(state.detail == item["detail"].string, "\(name)")
    #expect(state.isWorking == (item["isWorking"].bool == true), "\(name)")
    let kind: String
    switch state {
    case .hidden: kind = "hidden"
    case .choose(let listed, let failure):
      kind = "choose"
      let expected = item["methods"].array ?? []
      #expect(listed.count == expected.count, "\(name)")
      for (method, wanted) in zip(listed, expected) {
        #expect(method.id == wanted["id"].string, "\(name)")
        #expect(method.name == wanted["name"].string, "\(name)")
        #expect(method.detail == wanted["detail"].string, "\(name)")
        #expect(method.opensTerminal == (wanted["opensTerminal"].bool == true), "\(name)")
      }
      #expect(failure == item["chooseFailure"].string, "\(name)")
    case .waiting(let terminal):
      kind = "waiting"
      #expect(terminal == item["terminalPaneId"].string, "\(name)")
    case .verifying: kind = "verifying"
    case .signingOut: kind = "signingOut"
    case .failed(let reason):
      kind = "failed"
      #expect(reason == item["reason"].string, "\(name)")
    }
    #expect(kind == item["state"].string, "\(name)")
  }
  let words = all["signInLabels"]
  #expect(AgentSignInLabels.chooseTerminalHint == words["chooseTerminalHint"].string)
  #expect(AgentSignInLabels.showTerminal == words["showTerminal"].string)
  #expect(AgentSignInLabels.cancelSignIn == words["cancelSignIn"].string)
  #expect(AgentSignInLabels.dismiss == words["dismiss"].string)
}

@Test func aQuestionIsAFormOfTheSchemasProperties() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "elicitation") {
    let fields = AgentElicitationFields.fields(schema: item["schema"])
    let expected = item["fields"].array ?? []
    #expect(fields.count == expected.count, "\(item["name"].string ?? "")")
    for (field, wanted) in zip(fields, expected) {
      #expect(field.key == wanted["key"].string)
      #expect(field.title == wanted["title"].string)
    }
  }
  #expect(AgentElicitationFields.cancelTitle == all["elicitationLabels"]["cancel"].string)
  #expect(AgentElicitationFields.submitTitle == all["elicitationLabels"]["submit"].string)
}

@Test func subagentActivityNestsUnderItsRow() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "subagentGrouping") {
    let name = item["name"].string ?? ""
    let entries = AgentTranscriptGrouping.entries(
      try item["messages"].decoded([TranscriptMessage].self))
    #expect(shape(entries) == (item["shape"].array ?? []).compactMap(\.string), "\(name)")
    #expect(AgentTranscriptGrouping.awaited(entries) == item["awaited"].int, "\(name)")
  }
  for item in ContractVector.cases(all, "subagentGroupingEntryIds") {
    let entries = AgentTranscriptGrouping.entries(
      try item["messages"].decoded([TranscriptMessage].self))
    #expect(entries.map(\.id) == (item["ids"].array ?? []).compactMap(\.string))
  }
}

@Test func aSubagentRowSaysHowItGoes() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "subagentPresentation") {
    let name = item["name"].string ?? ""
    let run = try? item["run"].decoded(SubagentRun.self)
    let row = AgentSubagentPresentation(
      AgentSubagentGroup(subagentId: run?.subagentId ?? "", run: run, entries: []),
      now: try date(item["now"].string))
    let status: String =
      switch row.status {
      case .running: "running"
      case .succeeded: "succeeded"
      case .failed: "failed"
      case .stopped: "stopped"
      }
    #expect(status == item["status"].string, "\(name)")
    #expect(row.trailing == item["trailing"].string, "\(name)")
    #expect(row.model == item["model"].string, "\(name)")
    #expect(row.summary == item["summary"].string, "\(name)")
    if run == nil { #expect(row.name == all["subagentRunFallbackName"].string) }
  }
  for item in ContractVector.cases(all, "subagentWaiting") {
    #expect(AgentSubagentPresentation.waiting(item["count"].int ?? 0) == item["text"].string)
  }
}

@Test func thePreviewKeepsTheMeaningfulLinesAndCutsAtAWord() throws {
  for item in ContractVector.cases(try vector(), "subagentPreview") {
    let limit = item["limit"].int ?? 0
    #expect(
      AgentSubagentPreview.text(item["text"].string ?? "", limit: limit) == item["preview"].string,
      "\(item["text"].string ?? "")")
  }
}

@Test func theButtonForEarlierMessagesIsNamedAsOnTheDesktop() throws {
  #expect(
    AgentTranscriptLabels.loadEarlier == (try vector())["transcriptLabels"]["loadEarlier"].string)
}
