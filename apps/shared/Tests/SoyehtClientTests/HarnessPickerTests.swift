import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func installedReadyHarnessSortsBeforeEveryUnavailableEntry() throws {
  let unavailable = (0..<41).map { index in
    harness(id: "registry.agent-\(index)", name: "Agent \(index)", state: .not_installed)
  }
  let owned = harness(id: "registry.owned", name: "Owned agent", state: .ready)

  let sections = HarnessPickerModel.sections(unavailable + [owned])
  let ordered = sections.flatMap(\.items)

  #expect(ordered.count == 42)
  #expect(ordered.first?.id == owned.id)
  #expect(sections.first?.group == .ready)
  #expect(sections.last?.group == .unavailable)
  #expect(HarnessPickerModel.defaultHarness(in: unavailable + [owned])?.id == owned.id)
}

@Test func searchNarrowsFortyTwoHarnessesByNameSubstringWithoutHidingTheCatalogue() throws {
  let catalogue =
    (0..<41).map { index in
      harness(id: "registry.agent-\(index)", name: "Agent \(index)", state: .not_installed)
    } + [harness(id: "registry.target", name: "Distinct Target", state: .ready)]

  let all = HarnessPickerModel.sections(catalogue).flatMap(\.items)
  let match = HarnessPickerModel.sections(catalogue, query: "tinct tar").flatMap(\.items)

  #expect(all.count == 42)
  #expect(match.map(\.id) == ["registry.target"])
}

@Test func setupStatesStayLegibleAndDefaultNeverChoosesAnUnavailableHarness() throws {
  let signIn = harness(id: "sign-in", name: "Sign in", state: .sign_in)
  let check = harness(id: "check", name: "Check", state: .check_setup)
  let unavailable = harness(id: "missing", name: "Missing", state: .not_installed)

  #expect(HarnessPickerModel.statusLabel(for: signIn) == "Sign in")
  #expect(HarnessPickerModel.statusLabel(for: check) == "Check setup")
  #expect(HarnessPickerModel.statusLabel(for: unavailable) == "Not installed")
  #expect(HarnessPickerModel.defaultHarness(in: [unavailable, check])?.id == check.id)
  #expect(HarnessPickerModel.defaultHarness(in: [unavailable]) == nil)
}

private func harness(
  id: String, name: String, state: HarnessSetupState
) -> HarnessInfo {
  HarnessInfo(
    id: id, name: name, available: state != .not_installed, capabilities: [], options: [],
    authMethods: [], source: .built_in, setupState: state)
}
