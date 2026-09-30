import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let order = PaneSidebarOrder(workspaces: [["a1", "a2", "a3"], ["b1", "b2"], []])

@Test func sectionsGroupPanesUnderTheirWorkspaceInReplicaOrder() {
  let workspaces = ["left", "empty", "right"].map {
    Workspace(workspaceId: $0, name: $0, path: "/\($0)", layout: .object([:]))
  }
  let panes = [("r1", "right"), ("l1", "left"), ("r2", "right"), ("l2", "left")].map {
    Pane(
      paneId: $0.0, workspaceId: $0.1, name: $0.0, cwd: "/tmp", mode: .terminal, state: .idle,
      source: .protocol, confidence: 1, phrase: "", generation: 1, cols: 80, rows: 24,
      record: false)
  }

  let sections = PaneSidebarSection.sections(workspaces: workspaces, panes: panes)

  #expect(sections.map(\.id) == ["left", "empty", "right"])
  #expect(sections.map { $0.panes.map(\.paneId) } == [["l1", "l2"], [], ["r1", "r2"]])
  #expect(PaneSidebarOrder(sections).paneIds == ["l1", "l2", "r1", "r2"])
}

@Test func selectionSurvivesWhenItsPaneStillExists() {
  let next = PaneSidebarOrder(workspaces: [["a2", "a3"], ["b1", "b2", "b3"]])

  #expect(next.reconciled("b2", previous: order, preferred: "a2", fillsDetail: true) == "b2")
  #expect(next.reconciled("b2", previous: order, preferred: "a2", fillsDetail: false) == "b2")
}

@Test func closedPaneHandsOverToTheNextPaneInItsWorkspace() {
  let next = PaneSidebarOrder(workspaces: [["a1", "a3"], ["b1", "b2"]])

  #expect(next.reconciled("a2", previous: order, preferred: "b1", fillsDetail: true) == "a3")
}

@Test func closedLastPaneOfAWorkspaceHandsOverToItsPreviousSibling() {
  let next = PaneSidebarOrder(workspaces: [["a1", "a2"], ["b1", "b2"]])

  #expect(next.reconciled("a3", previous: order, preferred: "b2", fillsDetail: true) == "a2")
}

@Test func closedWorkspaceHandsOverToTheNearestPaneElsewhere() {
  let afterFirst = PaneSidebarOrder(workspaces: [["b1", "b2"]])
  let afterSecond = PaneSidebarOrder(workspaces: [["a1", "a2", "a3"]])

  #expect(
    afterFirst.reconciled("a2", previous: order, preferred: nil, fillsDetail: true) == "b1")
  #expect(
    afterSecond.reconciled("b1", previous: order, preferred: nil, fillsDetail: true) == "a3")
}

@Test func compactWidthReturnsToTheListWhenTheShownPaneCloses() {
  let next = PaneSidebarOrder(workspaces: [["a1", "a3"], ["b1", "b2"]])

  #expect(next.reconciled("a2", previous: order, preferred: "b1", fillsDetail: false) == nil)
  #expect(next.reconciled(nil, previous: order, preferred: "b1", fillsDetail: false) == nil)
}

@Test func emptySelectionTakesTheMacFocusThenTheFirstPane() {
  #expect(order.reconciled(nil, previous: order, preferred: "b2", fillsDetail: true) == "b2")
  #expect(order.reconciled(nil, previous: order, preferred: "gone", fillsDetail: true) == "a1")
  #expect(
    PaneSidebarOrder(workspaces: [[]]).reconciled(
      "a1", previous: order, preferred: nil, fillsDetail: true) == nil)
}

@Test func stepWrapsAcrossWorkspaces() {
  #expect(order.step(from: "a3", by: 1) == "b1")
  #expect(order.step(from: "b2", by: 1) == "a1")
  #expect(order.step(from: "a1", by: -1) == "b2")
  #expect(order.step(from: nil, by: 1) == "a1")
  #expect(order.step(from: "gone", by: -1) == "b2")
  #expect(PaneSidebarOrder(workspaces: []).step(from: nil, by: 1) == nil)
}

@Test func workspaceJumpKeepsASelectionAlreadyThere() {
  #expect(order.pane(inWorkspaceAt: 1, selection: "a2") == "b1")
  #expect(order.pane(inWorkspaceAt: 1, selection: "b2") == "b2")
  #expect(order.pane(inWorkspaceAt: 2, selection: "a2") == nil)
  #expect(order.pane(inWorkspaceAt: 8, selection: "a2") == nil)
}
