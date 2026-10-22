import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/slash-completion.json`, which the desktop client passes as well.
private func catalog(_ vector: JsonValue, _ name: String?) throws -> SlashCatalog {
  let entry = vector["catalogs"][name ?? ""]
  return SlashCatalog(
    skills: try entry["skills"].decoded([Skill].self),
    commands: try entry["commands"].decoded([AgentCommand].self))
}

private func source(_ name: String?) -> SlashSuggestion.Source {
  switch name {
  case "user": .skill(.user)
  case "workspace": .skill(.workspace)
  default: .command
  }
}

/// The names a case lists, in its order.
private func expectNames(_ found: [SlashSuggestion], of item: JsonValue) {
  let wanted = item["names"].array?.compactMap(\.string)
  #expect(found.map(\.name) == wanted, "\(item["name"].string ?? "")")
}

@Test func slashSharedCasesOfferTheSameSuggestionsInTheSameOrder() throws {
  let vector = try ContractVector.load("slash-completion")
  let cases = ContractVector.cases(vector, "suggestions")
  #expect(cases.count >= 2)
  for item in cases {
    let suggestions = try catalog(vector, item["catalog"].string).suggestions
    let expected = ContractVector.cases(item, "expected")
    #expect(suggestions.count == expected.count, "\(item["name"].string ?? "")")
    for (suggestion, want) in zip(suggestions, expected) {
      #expect(suggestion.name == want["name"].string)
      #expect(suggestion.detail == want["detail"].string)
      #expect(suggestion.hint == want["hint"].string)
      #expect(suggestion.source == source(want["source"].string))
    }
  }
}

@Test func slashSharedCasesReadTheQueryTheSameWay() throws {
  let vector = try ContractVector.load("slash-completion")
  let cases = ContractVector.cases(vector, "queries")
  #expect(cases.count >= 8)
  for item in cases {
    let text = item["text"].string ?? ""
    #expect(SlashCatalog.query(in: text) == item["query"].string, "\(text)")
  }
}

@Test func slashSharedCasesFilterAndOrderTheSameWay() throws {
  let vector = try ContractVector.load("slash-completion")
  let cases = ContractVector.cases(vector, "matches")
  #expect(cases.count >= 7)
  for item in cases {
    let found = try catalog(vector, item["catalog"].string).matches(item["query"].string ?? "")
    expectNames(found, of: item)
  }
}

@Test func slashSharedCasesLeaveTheSameTextWhenAnEntryIsChosen() throws {
  let vector = try ContractVector.load("slash-completion")
  for item in ContractVector.cases(vector, "accepts") {
    let suggestion = SlashSuggestion(
      name: item["name"].string ?? "", detail: "", hint: nil, source: source(item["source"].string))
    let accepted = SlashCatalog.accept(suggestion)
    #expect(accepted.text == item["text"].string)
    #expect(accepted.cursor == accepted.text.count)
  }
}

@MainActor @Test func slashSharedCasesShowTheSameRows() throws {
  let vector = try ContractVector.load("slash-completion")
  #expect(SlashCompletionState.visibleRows == vector["visibleRows"].int)
  for item in ContractVector.cases(vector, "shown") {
    let all = try catalog(vector, item["catalog"].string)
    let query = SlashCatalog.query(in: item["text"].string ?? "")
    let shown = query.map { Array(all.matches($0).prefix(SlashCompletionState.visibleRows)) } ?? []
    expectNames(shown, of: item)
  }
}

@Test func slashSharedCasesSendTheSamePrompt() throws {
  let vector = try ContractVector.load("slash-completion")
  let cases = ContractVector.cases(vector, "prompts")
  #expect(cases.count >= 11)
  for item in cases {
    let text = item["text"].string ?? ""
    #expect(try catalog(vector, item["catalog"].string).prompt(for: text) == item["prompt"].string)
  }
}
