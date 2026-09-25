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

@Test func setupStatesStayLegibleAndOnlyObservedReadyGetsAutomaticDefault() throws {
  let signIn = harness(id: "sign-in", name: "Sign in", state: .sign_in)
  let check = harness(id: "check", name: "Check", state: .check_setup)
  let unavailable = harness(id: "missing", name: "Missing", state: .not_installed)

  #expect(HarnessPickerModel.statusLabel(for: signIn) == "Sign in")
  #expect(HarnessPickerModel.statusLabel(for: check) == "Check setup")
  #expect(HarnessPickerModel.statusLabel(for: unavailable) == "Not installed")
  #expect(HarnessPickerModel.defaultHarness(in: [unavailable, check]) == nil)
  #expect(HarnessPickerModel.defaultHarness(in: [unavailable]) == nil)
  #expect(
    HarnessPickerModel.defaultHarness(in: [check, unavailable], preferredId: check.id)?.id
      == check.id)
}

@Test func verifiedReadyOutranksUnverifiedRegistryAdapters() throws {
  let runnerBackedStrangers = (0..<26).map { index in
    harness(
      id: "registry.runner-\(index)", name: "Agent \(index)", state: .not_checked,
      installed: false)
  }
  let codex = harness(
    id: "registry.codex-acp", name: "Codex", state: .ready, installed: true)

  let sections = HarnessPickerModel.sections(runnerBackedStrangers + [codex])
  let ordered = sections.flatMap(\.items)

  #expect(ordered.count == 27)
  #expect(ordered.first?.id == codex.id)
  #expect(sections.first?.group == .ready)
  #expect(sections.last?.group == .notChecked)
  #expect(HarnessPickerModel.defaultHarness(in: runnerBackedStrangers + [codex])?.id == codex.id)
}

@Test func readinessOrderingAndLastSuccessfulSessionChooseDefault() {
  var older = harness(id: "older", name: "A", state: .ready)
  older.lastReadyAtMs = "100"
  var newer = harness(id: "newer", name: "B", state: .ready)
  newer.lastReadyAtMs = "200"
  let unknown = harness(id: "unknown", name: "C", state: .not_checked)
  let login = harness(id: "login", name: "D", state: .sign_in)
  let failed = harness(id: "failed", name: "E", state: .check_setup)
  let missing = harness(id: "missing", name: "F", state: .not_installed)

  #expect(
    HarnessPickerModel.sections([missing, failed, login, unknown, newer, older])
      .map(\.group) == [.ready, .notChecked, .signIn, .checkSetup, .unavailable])
  #expect(HarnessPickerModel.defaultHarness(in: [older, newer])?.id == newer.id)
  #expect(
    HarnessPickerModel.defaultHarness(in: [older, newer], preferredId: older.id)?.id == older.id)
  #expect(HarnessPickerModel.statusLabel(for: unknown) == "Not checked")
  #expect(HarnessPickerModel.statusReason(for: unknown).contains("not been started"))
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
    options: [], authMethods: [], source: .built_in, setupState: state,
    setupReason: installed == false ? .registry_package_unverified : nil)
}
