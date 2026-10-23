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

private struct HeldMessage: Equatable {
  let localId: String
  let text: String
  let complete: Bool
  let status: String?
  let terminalBytes: String?
}
