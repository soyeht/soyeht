import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The `merge` cases of `contracts/vectors/agent-transcript.json`, which the desktop client
/// passes as well: what the replica holds after a series of updates and pages. The `ticker`
/// cases are in `AgentPaneTickerTests`.
@Test func theReplicaHoldsWhatTheVectorSaysAfterEachSeriesOfUpdates() throws {
  for item in ContractVector.cases(try ContractVector.load("agent-transcript"), "merge") {
    var view = AgentPaneView()
    for step in item["steps"].array ?? [] {
      if step["update"] != .null {
        view.merge(try step["update"].decoded(TranscriptMessage.self))
      } else {
        view.merge(try step["page"].decoded([TranscriptMessage].self))
      }
    }
    let held = view.messages.map { message in
      HeldMessage(
        localId: message.localId, text: message.text, complete: message.complete,
        status: message.tool?.status.rawValue, terminalBytes: message.terminal?.bytes)
    }
    let expected = (item["messages"].array ?? []).map { entry in
      HeldMessage(
        localId: entry["localId"].string ?? "", text: entry["text"].string ?? "",
        complete: entry["complete"].bool ?? false, status: entry["status"].string,
        terminalBytes: entry["terminalBytes"].string)
    }
    #expect(held == expected, "\(item["name"].string ?? "")")
  }
}

@Test func theVectorListsAsManyEarlierToolCallsAsThePolicyKeeps() throws {
  let cases = ContractVector.cases(try ContractVector.load("agent-transcript"), "ticker")
  let capped = try #require(cases.first { $0["name"].string?.contains("no more") == true })
  #expect(capped["items"].array?.count == 1 + ClientPolicy.capsTickerHistory)
}

@Test func theReplicaLetsGoOfWhatTheVectorSaysItDoesNotNeed() throws {
  let vector = try ContractVector.load("agent-transcript")
  for item in ContractVector.cases(vector, "window") {
    var view = AgentPaneView()
    view.messages = (item["messages"].array ?? []).enumerated().map { windowMessage($1, $0 + 1) }
    view.nextTranscriptCursor = item["older"].string
    let limits = AgentTranscriptWindow.Limits(
      held: Int(vector["windowLimits"]["held"].double ?? 0),
      offscreen: Int(vector["windowLimits"]["offscreen"].double ?? 0),
      slack: Int(vector["windowLimits"]["slack"].double ?? 0))
    view.trim(
      shown: item["shown"].bool == true, viewing: item["viewing"].string, limits: limits,
      force: item["merged"].bool != true)
    let name = item["name"].string ?? ""
    #expect(
      view.messages.map(\.localId) == (item["kept"].array ?? []).compactMap(\.string), "\(name)")
    #expect(view.nextTranscriptCursor == item["keptOlder"].string, "\(name)")
  }
}

@Test func theCursorAfterAPageIsTheOlderOfTheOneHeldAndThePages() throws {
  for item in ContractVector.cases(try ContractVector.load("agent-transcript"), "cursor") {
    let newest = item["newestHeld"].double.map { UInt64($0) }
    let cursor = AgentTranscriptWindow.older(
      item["held"].string, item["page"].string, newestHeld: newest)
    #expect(cursor == item["expect"].string, "\(item["name"].string ?? "")")
  }
}

@Test func aPaneReadsTheNewestPageWhenTheVectorSaysItDoes() throws {
  let vector = try ContractVector.load("agent-transcript")
  for item in ContractVector.cases(vector, "refill") {
    let wants = AgentTranscriptWindow.wantsNewestPage(
      shown: item["shown"].bool == true, stale: item["stale"].bool == true,
      held: Int(item["held"].double ?? 0), hasOlder: item["older"].bool == true,
      page: Int(vector["windowLimits"]["slack"].double ?? 0))
    #expect(wants == item["expect"].bool, "\(item["name"].string ?? "")")
  }
}

@Test func aPaneThatIsNotShownHoldsOnlyItsWindowHoweverMuchArrives() {
  var view = AgentPaneView()
  let tool = ToolCall(
    toolCallId: "t", title: "ls", status: .completed, content: .string("tree"),
    locations: [ToolLocation(path: "a")], rawInput: .string("in"), rawOutput: .string("out"))
  for index in 1...3_000 {
    view.merge(windowMessage(.string("m\(index)"), index, tool: index == 3_000 ? tool : nil))
  }
  let page = Int(ClientPolicy.pageSizeTranscript)
  #expect(view.messages.count <= ClientPolicy.capsOffscreenMessages + page)
  #expect(view.nextTranscriptCursor == view.messages.first?.seq, "load earlier fetches the rest")
  #expect(view.revisions.count == view.messages.count)
  let kept = view.messages.last?.tool
  #expect(kept?.locations.count == 1 && kept?.content == nil && kept?.rawInput == nil)
  view.trimToWindow(force: true)
  #expect(view.messages.count == ClientPolicy.capsOffscreenMessages, "leaving the screen trims")
  view.scroll.isShown = true
  for index in 3_001...6_000 { view.merge(windowMessage(.string("m\(index)"), index)) }
  #expect(view.messages.count <= ClientPolicy.capsHeldMessages + page)
}

/// A message of the vector's compact form: a name, or an object that makes it a streaming reply,
/// a tool call, a usage report or an unfinished command output.
private func windowMessage(_ entry: JsonValue, _ seq: Int, tool: ToolCall? = nil)
  -> TranscriptMessage
{
  let id = entry.string ?? entry["id"].string ?? ""
  let status = entry["tool"].string.flatMap(ToolStatus.init(rawValue:))
  let user = entry["user"].bool == true
  let kind: TranscriptKind =
    status != nil || tool != nil
    ? .tool
    : entry["usage"].bool == true ? .usage : entry["terminal"].bool == true ? .terminal : .message
  let finished = ![.pending, .in_progress].contains(status) && entry["streaming"].bool != true
  return TranscriptMessage(
    localId: id, generation: 1, seq: "\(seq)", role: user ? .user : .assistant, kind: kind,
    text: "",
    complete: finished && kind != .terminal,
    tool: tool ?? status.map { ToolCall(toolCallId: id, title: id, status: $0, locations: []) })
}

private struct HeldMessage: Equatable {
  let localId: String
  let text: String
  let complete: Bool
  let status: String?
  let terminalBytes: String?
}
