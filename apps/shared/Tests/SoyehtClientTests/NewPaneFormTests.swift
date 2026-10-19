import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func harness(_ id: String, name: String? = nil, available: Bool = true) -> HarnessInfo {
  HarnessInfo(
    id: id, name: name ?? id, available: available, capabilities: [], options: [],
    authMethods: [], setupState: available ? .ready : .not_installed)
}

@Test func aTerminalNeedsOnlyAFolder() {
  var form = NewPaneForm(kind: .terminal)
  #expect(form.blocker(harnesses: []) == .noFolder)
  form.directory = "/Users/me/project"
  #expect(form.blocker(harnesses: []) == nil)
  #expect(form.canCreate(harnesses: []))
}

@Test func anAgentNeedsAReadyHarnessAndAShortFirstMessage() {
  let harnesses = [harness("ready"), harness("missing", available: false)]
  var form = NewPaneForm(kind: .agent, directory: "/repo")
  #expect(form.blocker(harnesses: harnesses) == .noAgent)
  form.harness = "missing"
  #expect(form.blocker(harnesses: harnesses) == .agentUnavailable)
  form.harness = "ready"
  #expect(form.blocker(harnesses: harnesses) == nil)
  form.prompt = String(repeating: "a", count: ProtocolLimits.promptBytes + 1)
  #expect(form.blocker(harnesses: harnesses) == .promptTooLong)
  form.prompt = "  hello \n"
  #expect(form.firstPrompt == "hello")
  #expect(form.blocker(harnesses: harnesses) == nil)
}

@Test func aWritingPaneNeedsATitleAndNeverCreatesTwice() {
  var form = NewPaneForm(kind: .writing(.texts), directory: "/repo")
  #expect(form.blocker(harnesses: []) == .noTitle)
  form.title = "   "
  #expect(form.blocker(harnesses: []) == .noTitle)
  form.title = "Chapter one"
  #expect(form.canCreate(harnesses: []))
  #expect(!form.canCreate(harnesses: [], submitting: true))
}

@Test func choosingAnotherHarnessDropsTheModelEffortAndModeOfTheOldOne() {
  var form = NewPaneForm()
  form.chooseHarness("a")
  form.model = "big"
  form.effort = "high"
  form.permissionMode = "plan"
  form.chooseHarness("a")
  #expect(form.model == "big")
  form.chooseHarness("b")
  #expect(form.model.isEmpty && form.effort.isEmpty && form.permissionMode == nil)
  #expect(form.harness == "b")
}

@Test func theFormKeepsAnAvailableHarnessAndOtherwiseTakesThePreferredOne() {
  let harnesses = [harness("a"), harness("b")]
  var form = NewPaneForm()
  form.keepOrChooseHarness(in: harnesses, preferredId: "b")
  #expect(form.harness == "b")
  form.keepOrChooseHarness(in: harnesses, preferredId: "a")
  #expect(form.harness == "b")
  form.model = "m"
  form.keepOrChooseHarness(in: [harness("b", available: false), harness("a")], preferredId: nil)
  #expect(form.harness == "a")
  #expect(form.model.isEmpty)
  form.keepOrChooseHarness(in: [], preferredId: nil)
  #expect(form.harness == "a")
}

@Test func theAgentLabelPrefersTheModelThenTheHarness() {
  let harnesses = [harness("a", name: "Agent A")]
  var form = NewPaneForm()
  #expect(form.agentLabel(harnesses: harnesses, catalogue: nil) == "Choose agent")
  form.harness = "a"
  #expect(form.agentLabel(harnesses: harnesses, catalogue: nil) == "Agent A")
  form.model = "m1"
  let catalogue = AgentCatalogue(models: [ModelInfo(id: "m1", name: "Model One")])
  #expect(form.agentLabel(harnesses: harnesses, catalogue: catalogue) == "Model One")
}

@MainActor @Test func createPaneSendsTheFormsFolderHarnessAndChoices() async throws {
  let client = EngineClient(socket: "/tmp/new-pane-form.sock")
  var seen: [(RequestMethod, JsonValue)] = []
  client.requestStub = { method, params in
    seen.append((method, params))
    switch method {
    case .workspaceCreate:
      return try .object([
        "workspace": .encoded(
          Workspace(
            workspaceId: "w1", name: "project", path: "/Users/me/project",
            layout: .object([:])))
      ])
    case .paneCreate: return .object(["paneId": .string("p1")])
    default: throw ClientFailure.invalidFrame
    }
  }
  var form = NewPaneForm(kind: .agent, directory: "/Users/me/project")
  form.harness = "h"
  form.model = "m"
  form.permissionMode = "plan"

  let paneId = try await client.createPane(form)

  #expect(paneId == "p1")
  #expect(seen.map(\.0) == [.workspaceCreate, .paneCreate])
  let created = seen[1].1
  #expect(created["workspaceId"].string == "w1")
  #expect(created["mode"].string == PaneMode.agent.rawValue)
  #expect(created["cwd"].string == "/Users/me/project")
  #expect(created["harness"].string == "h")
  #expect(created["model"].string == "m")
  #expect(created["effort"].string == nil)
  #expect(created["permissionMode"].string == "plan")
}
