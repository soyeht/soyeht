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

@Test func sharedPackageRunnerCannotOutrankLocallyInstalledHarness() throws {
  let runnerBackedStrangers = (0..<26).map { index in
    harness(
      id: "registry.runner-\(index)", name: "Agent \(index)", state: .check_setup,
      installed: false)
  }
  let codex = harness(
    id: "registry.codex-acp", name: "Codex", state: .check_setup, installed: true)

  let sections = HarnessPickerModel.sections(runnerBackedStrangers + [codex])
  let ordered = sections.flatMap(\.items)

  #expect(ordered.count == 27)
  #expect(ordered.first?.id == codex.id)
  #expect(sections.first?.group == .needsSetup)
  #expect(sections.last?.group == .catalogue)
  #expect(HarnessPickerModel.defaultHarness(in: runnerBackedStrangers + [codex])?.id == codex.id)
}

@Test func fetchedPickerStatesNeverDescribeLoadingAsAnEmptySearch() {
  let owned = harness(id: "registry.owned", name: "Owned agent", state: .ready)

  #expect(
    HarnessPickerModel.contentState(harnesses: [], query: "", loading: true) == .loading)
  #expect(
    HarnessPickerModel.contentState(harnesses: [], query: "", loading: false)
      == .emptyCatalogue)
  #expect(
    HarnessPickerModel.contentState(harnesses: [owned], query: "missing", loading: false)
      == .noMatches)
  #expect(
    HarnessPickerModel.contentState(harnesses: [owned], query: "", loading: false) == .results)

  #expect(HarnessPickerModel.modelContentState(catalogue: nil, loading: true) == .loading)
  #expect(HarnessPickerModel.modelContentState(catalogue: nil, loading: false) == .unavailable)
  #expect(
    HarnessPickerModel.modelContentState(catalogue: AgentCatalogue(), loading: false) == .empty)
  #expect(
    HarnessPickerModel.modelContentState(
      catalogue: AgentCatalogue(
        models: [ModelInfo(id: "model", name: "Model")]), loading: false)
      == .results)
  #expect(HarnessPickerModel.matches(owned, query: "owned"))
  #expect(!HarnessPickerModel.matches(owned, query: "another"))
}

@Test func observedModelRefusalStaysVisibleWithoutHidingTheChoice() {
  let refused = ModelInfo(
    id: "model-a", name: "Model A",
    observedRefusal: ModelRefusal(reason: "Model is disabled", observedAt: "2026-09-15T21:00:00Z"))

  #expect(HarnessPickerModel.modelLabel(for: refused) == "Model A — refused")
  #expect(HarnessPickerModel.modelRefusalReason(for: refused) == "Model is disabled")
  #expect(
    HarnessPickerModel.modelLabel(for: ModelInfo(id: "model-b", name: "Model B")) == "Model B")
}

private func harness(
  id: String, name: String, state: HarnessSetupState, installed: Bool? = nil
) -> HarnessInfo {
  HarnessInfo(
    id: id, name: name, available: state != .not_installed, installed: installed, capabilities: [],
    options: [], authMethods: [], source: .built_in, setupState: state)
}
