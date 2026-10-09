import Testing

@testable import SoyehtClient

@MainActor
@Test func aSlowCatalogueLoadIsAnInlineStateNotAModalAlert() async {
  let client = EngineClient(socket: "/tmp/background-load-timeout.sock")
  await client.performBackground(.catalogue(harness: "registry.opencode")) {
    throw ClientFailure.requestTimeout("model.list")
  }.value
  #expect(client.problem == nil)
  #expect(
    client.loadFailure(.catalogue(harness: "registry.opencode"))
      == "The engine did not respond to model.list in time.")
  #expect(client.loadFailure(.catalogue(harness: "registry.codex-acp")) == nil)
  #expect(client.loadFailure(.harnesses) == nil)
}

@MainActor
@Test func aLoadThatSucceedsAfterFailingClearsItsFailure() async {
  let client = EngineClient(socket: "/tmp/background-load-retry.sock")
  await client.performBackground(.harnesses) { throw ClientFailure.requestTimeout("harness.list") }
    .value
  #expect(client.loadFailure(.harnesses) != nil)
  #expect(client.harnessListStalled)
  await client.performBackground(.harnesses) {}.value
  #expect(client.loadFailure(.harnesses) == nil)
  #expect(!client.harnessListStalled)
  #expect(client.problem == nil)
}

@MainActor
@Test func aCancelledLoadLeavesNoFailureBehind() async {
  let client = EngineClient(socket: "/tmp/background-load-cancel.sock")
  await client.performBackground(.customHarnesses) { throw CancellationError() }.value
  #expect(client.loadFailure(.customHarnesses) == nil)
  #expect(client.problem == nil)
}

@MainActor
@Test func preferencesLoadsFailInlineWhenTheEngineIsNotConnected() async {
  let client = EngineClient(socket: "/tmp/background-load-offline.sock")
  client.refreshHarnesses()
  client.refreshCustomHarnesses()
  client.refreshCatalogue(harness: "registry.opencode")
  client.refreshDefaultPermissionModes()
  for _ in 0..<200
  where client.loadFailure(.catalogue(harness: "registry.opencode")) == nil
    || client.loadFailure(.harnesses) == nil || client.loadFailure(.customHarnesses) == nil
  {
    try? await Task.sleep(for: .milliseconds(10))
  }
  #expect(client.loadFailure(.harnesses) != nil)
  #expect(client.loadFailure(.customHarnesses) != nil)
  #expect(client.loadFailure(.catalogue(harness: "registry.opencode")) != nil)
  #expect(client.problem == nil)
  #expect(client.harnessListStalled)
  #expect(!client.harnessesLoading)
  #expect(!client.agentCatalogueLoading("registry.opencode"))
}

@MainActor
@Test func userActionsStillReportTheirFailureInTheAlert() async {
  let client = EngineClient(socket: "/tmp/background-load-action.sock")
  client.perform { throw ClientFailure.requestTimeout("pane.close") }
  for _ in 0..<200 where client.problem == nil {
    try? await Task.sleep(for: .milliseconds(10))
  }
  #expect(client.problem == "The engine did not respond to pane.close in time.")
}

/// Holds a load open until the test lets it finish.
@MainActor
private final class LoadGate {
  private var released = false

  func release() { released = true }

  func wait() async {
    while !released { await Task.yield() }
  }
}

@MainActor
@Test func aLateFailureOfASupersededLoadDoesNotOverwriteANewerSuccess() async {
  let client = EngineClient(socket: "/tmp/background-load-late-failure.sock")
  let gate = LoadGate()
  let older = client.performBackground(.catalogue(harness: "registry.opencode")) {
    await gate.wait()
    throw ClientFailure.requestTimeout("model.list")
  }
  await client.performBackground(.catalogue(harness: "registry.opencode")) {}.value
  gate.release()
  await older.value
  #expect(client.loadFailure(.catalogue(harness: "registry.opencode")) == nil)
}

@MainActor
@Test func aLateSuccessOfASupersededLoadDoesNotHideANewerFailure() async {
  let client = EngineClient(socket: "/tmp/background-load-late-success.sock")
  let gate = LoadGate()
  let older = client.performBackground(.customHarnesses) { await gate.wait() }
  await client.performBackground(.customHarnesses) {
    throw ClientFailure.requestTimeout("harness.custom.list")
  }.value
  gate.release()
  await older.value
  #expect(client.loadFailure(.customHarnesses) != nil)
}

@MainActor
@Test func loadsOfDifferentListsDoNotSupersedeEachOther() async {
  let client = EngineClient(socket: "/tmp/background-load-independent.sock")
  let gate = LoadGate()
  let slow = client.performBackground(.defaultPermissionModes) {
    await gate.wait()
    throw ClientFailure.requestTimeout("settings.get")
  }
  await client.performBackground(.customHarnesses) {}.value
  gate.release()
  await slow.value
  #expect(client.loadFailure(.defaultPermissionModes) != nil)
  #expect(client.loadFailure(.customHarnesses) == nil)
}

@Test func everyLoadHasItsOwnFailureText() {
  let loads: [BackgroundLoad] = [
    .harnesses, .customHarnesses, .defaultPermissionModes, .catalogue(harness: "registry.opencode"),
  ]
  #expect(Set(loads.map(\.failureText)).count == loads.count)
  #expect(loads.allSatisfy { !$0.failureText.isEmpty })
}

@Test func aFailedLoadShowsInPlaceOfAnEmptyList() {
  #expect(
    HarnessPickerModel.contentState(harnesses: [], query: "", loading: false, failed: true)
      == .failed)
  #expect(
    HarnessPickerModel.contentState(harnesses: [], query: "", loading: true, failed: true)
      == .loading)
  #expect(
    HarnessPickerModel.modelContentState(catalogue: nil, loading: false, failed: true) == .failed)
  #expect(
    HarnessPickerModel.modelContentState(catalogue: nil, loading: true, failed: true) == .loading)
  #expect(
    HarnessPickerModel.modelContentState(catalogue: AgentCatalogue(), loading: false, failed: true)
      == .empty)
}
