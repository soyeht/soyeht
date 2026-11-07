import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func entry(_ workspaceId: String, _ trust: ProjectTrust) -> ProjectTrustEntry {
  ProjectTrustEntry(workspaceId: workspaceId, trust: trust)
}

private func changed(_ ids: [String], _ trust: ProjectTrust) -> ProjectTrustChangedEvent {
  ProjectTrustChangedEvent(
    eventId: "1", at: "2026-11-06T12:00:00Z", workspaceIds: ids, trust: trust)
}

@MainActor @Test func trustLoadsFromTheSnapshotAndReplacesWhatWasThere() {
  let replica = ProjectTrustReplica()
  replica.load([entry("a", .undecided), entry("b", .trusted)])
  #expect(replica.trust(for: "a") == .undecided)
  #expect(replica.trust(for: "b") == .trusted)
  replica.load([entry("b", .restricted)])
  #expect(replica.trust(for: "a") == nil)
  #expect(replica.trust(for: "b") == .restricted)
}

@MainActor @Test func theChangedEventSetsEveryWorkspaceOverTheSameFolder() {
  let replica = ProjectTrustReplica()
  replica.load([entry("a", .undecided), entry("b", .undecided)])
  replica.apply(changed(["a", "b", "late"], .trusted))
  #expect(replica.trust(for: "a") == .trusted)
  #expect(replica.trust(for: "b") == .trusted)
  // The engine sends the answer right after `workspace.created`, before any snapshot.
  #expect(replica.trust(for: "late") == .trusted)
  #expect(!replica.needsDecision("a"))
}

@MainActor @Test func undecidedBehavesAsRestrictedAndAnUnlistedWorkspaceIsNotClaimed() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided), entry("r", .restricted), entry("t", .trusted)])
  #expect(replica.isRestricted("u"))
  #expect(replica.isRestricted("r"))
  #expect(!replica.isRestricted("t"))
  #expect(!replica.isRestricted("unknown"))
  #expect(!replica.needsDecision("unknown"))
}

@MainActor @Test func onlyAnUndecidedFolderNeedsADecision() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided), entry("r", .restricted), entry("t", .trusted)])
  #expect(replica.needsDecision("u"))
  #expect(!replica.needsDecision("r"))
  #expect(!replica.needsDecision("t"))
}

@MainActor @Test func notNowDefersUntilRelaunchEvenAcrossADisconnect() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided)])
  replica.putOff("u")
  #expect(!replica.needsDecision("u"))
  #expect(replica.promptedWorkspace(selected: "u") == nil)
  replica.reset()
  replica.load([entry("u", .undecided)])
  #expect(!replica.needsDecision("u"))
}

@MainActor @Test func resetForgetsTheEnginesAnswersAndTheReviewRequest() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided)])
  replica.requestReview("u")
  replica.reset()
  #expect(replica.entries.isEmpty)
  #expect(replica.requested == nil)
}

@MainActor @Test func theSheetAsksAboutTheSelectedWorkspaceOnceAndAReviewReopensIt() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided), entry("r", .restricted)])
  #expect(replica.promptedWorkspace(selected: "u") == "u")
  #expect(replica.promptedWorkspace(selected: "r") == nil)
  #expect(replica.promptedWorkspace(selected: nil) == nil)

  replica.putOff("u")
  #expect(replica.promptedWorkspace(selected: "u") == nil)
  replica.requestReview("r")
  #expect(replica.promptedWorkspace(selected: "u") == "r")
  replica.requestReview("u")
  #expect(replica.needsDecision("u"))

  replica.apply(changed(["u"], .restricted))
  #expect(replica.requested == nil)
  #expect(replica.promptedWorkspace(selected: "u") == nil)
}

@MainActor @Test func closingAWorkspaceDropsItsTrust() {
  let replica = ProjectTrustReplica()
  replica.load([entry("u", .undecided)])
  replica.putOff("u")
  replica.forget("u")
  #expect(replica.trust(for: "u") == nil)
  replica.load([entry("u", .undecided)])
  #expect(replica.needsDecision("u"))
}

@MainActor @Test func theClientOwnsTheReplicaAndResetsItOnADisconnect() {
  let client = EngineClient(socket: "/tmp/project-trust-replicas.sock")
  #expect(client.replicas.contains { $0 === client.projectTrust })
  client.projectTrust.load([entry("u", .undecided)])
  for replica in client.replicas { replica.reset() }
  #expect(client.projectTrust.entries.isEmpty)
}

@MainActor @Test func theSnapshotAssemblyKeepsTheTrustSection() throws {
  var assembly = SnapshotAssembly()
  let items = [entry("a", .undecided), entry("b", .trusted)]
  let data = JsonValue.array(try items.map { try .encoded($0) })
  try assembly.append(
    StateSnapshotPartEvent(snapshotId: "s", index: 0, of: 1, section: .trust, data: data))
  let sections = try assembly.finish(StateSnapshotResult(snapshotId: "s", cursor: "3", parts: 1))
  let client = EngineClient(socket: "/tmp/project-trust-snapshot.sock")
  try client.applyProjectTrustSnapshot(sections)
  #expect(client.projectTrust.trust(for: "a") == .undecided)
  #expect(client.projectTrust.trust(for: "b") == .trusted)

  // A snapshot with no trust section (an engine that predates it) clears the old answers.
  try client.applyProjectTrustSnapshot([:])
  #expect(client.projectTrust.entries.isEmpty)
}

@MainActor @Test func theChangedEventDecodesFromTheWireParams() throws {
  let client = EngineClient(socket: "/tmp/project-trust-event.sock")
  try client.applyProjectTrustEvent(try .encoded(changed(["a"], .restricted)))
  #expect(client.projectTrust.trust(for: "a") == .restricted)
  #expect(throws: (any Error).self) {
    try client.applyProjectTrustEvent(.object(["workspaceIds": .array([]), "trust": .string("?")]))
  }
}

@MainActor @Test func theSetRequestSendsTheContractParamsAndAppliesTheResult() async throws {
  let client = EngineClient(socket: "/tmp/project-trust-set.sock")
  client.projectTrust.load([entry("a", .undecided), entry("b", .undecided)])
  var seen: [(RequestMethod, JsonValue)] = []
  client.requestStub = { method, params in
    seen.append((method, params))
    return try .encoded(ProjectTrustSetResult(workspaceIds: ["a", "b"], trust: .trusted))
  }

  let accepted = await client.setProjectTrust(workspaceId: "a", to: .trusted)

  #expect(accepted)
  #expect(seen.count == 1)
  #expect(seen[0].0 == RequestMethod.projectTrustSet)
  let params = try seen[0].1.decoded(ProjectTrustSetParams.self)
  #expect(params.workspaceId == "a")
  #expect(params.trust == .trusted)
  #expect(!params.idempotencyKey.isEmpty)
  #expect(client.projectTrust.trust(for: "a") == .trusted)
  #expect(client.projectTrust.trust(for: "b") == .trusted)
  #expect(client.problem == nil)
}

@MainActor @Test func eachAnswerCarriesItsOwnIdempotencyKey() async throws {
  let client = EngineClient(socket: "/tmp/project-trust-keys.sock")
  var keys: [String] = []
  client.requestStub = { _, params in
    keys.append(params["idempotencyKey"].string ?? "")
    return try .encoded(ProjectTrustSetResult(workspaceIds: ["a"], trust: .restricted))
  }
  await client.setProjectTrust(workspaceId: "a", to: .restricted)
  await client.setProjectTrust(workspaceId: "a", to: .restricted)
  #expect(keys.count == 2)
  #expect(Set(keys).count == 2)
}

@MainActor @Test func aRefusedAnswerShowsThePolicyTextAndDoesNotAskAgain() async {
  let client = EngineClient(socket: "/tmp/project-trust-refused.sock")
  client.projectTrust.load([entry("a", .undecided)])
  client.requestStub = { _, _ in
    throw ClientFailure.remote(ErrorKind.forbidden_role.rawValue, "no")
  }

  let accepted = await client.setProjectTrust(workspaceId: "a", to: .trusted)

  #expect(!accepted)
  #expect(client.problem == ClientPolicy.projectTrustFailed)
  #expect(client.projectTrust.trust(for: "a") == .undecided)
  #expect(!client.projectTrust.needsDecision("a"))
}

@Test func theSheetSaysWhatTheClientPolicySays() {
  #expect(ProjectTrustPresentation.title == ClientPolicy.projectTrustTitle)
  #expect(ProjectTrustPresentation.summary == ClientPolicy.projectTrustSummary)
  #expect(
    ProjectTrustPresentation.choices.map(\.label)
      == [ClientPolicy.projectTrustRestrict, ClientPolicy.projectTrustTrust])
}

@Test func keepingTheFolderRestrictedIsTheOnlyDefault() {
  let defaults = ProjectTrustPresentation.choices.filter(\.isDefault)
  #expect(defaults.map(\.choice) == [.restricted])
  #expect(ProjectTrustPresentation.choices.map(\.choice) == [.restricted, .trusted])
}

private func server(
  _ state: LspState, detail: String? = nil, safeMode: Bool? = nil
) -> LspServerStatus {
  LspServerStatus(
    language: .rust, state: state, generation: 1, detail: detail, safeMode: safeMode)
}

@Test func theLanguageMarkerKeepsItsLineForAServerThatIsNotRestricted() {
  #expect(ProjectTrustPresentation.languageServerHelp(nil) == "No language server is active")
  #expect(
    ProjectTrustPresentation.languageServerHelp(server(.ready)) == "Language server: ready")
  #expect(
    ProjectTrustPresentation.languageServerHelp(server(.starting, detail: "indexing"))
      == "Language server: starting · indexing")
  #expect(
    ProjectTrustPresentation.languageServerHelp(server(.ready, safeMode: false))
      == "Language server: ready")
}

@Test func aRestrictedServerOrOneInSafeModeSaysWhy() {
  let restricted = ProjectTrustPresentation.languageServerHelp(
    server(.restricted, detail: "folder not trusted"))
  #expect(restricted.hasPrefix("Language server: restricted · folder not trusted"))
  #expect(restricted.hasSuffix(ClientPolicy.projectTrustRestrictedNote))
  let safe = ProjectTrustPresentation.languageServerHelp(server(.ready, safeMode: true))
  #expect(safe.hasPrefix("Language server: ready"))
  #expect(safe.hasSuffix(ClientPolicy.projectTrustRestrictedNote))
}
