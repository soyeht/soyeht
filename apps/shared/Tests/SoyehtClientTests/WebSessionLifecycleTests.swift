import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func pane(
  _ id: String, mode: PaneMode = .web, incognito: Bool? = nil, workspace: String = "w"
) -> Pane {
  var pane = PaneLayoutFixture.pane(id, workspace: workspace, mode: mode)
  pane.incognito = incognito
  return pane
}

@Test func aSessionOfALiveWebPaneOfTheSameKindContinues() {
  #expect(
    WebSessionLifecycle.ended(
      sessions: ["a": false, "b": true], panes: [pane("a"), pane("b", incognito: true)]
    ).isEmpty)
  #expect(
    WebSessionLifecycle.ended(sessions: ["a": false], panes: [pane("a", incognito: false)]).isEmpty)
}

@Test func aSessionEndsWhenItsPaneIsGoneOrIsNoLongerAWebPane() {
  let ended = WebSessionLifecycle.ended(
    sessions: ["gone": false, "terminal": false, "web": false],
    panes: [pane("terminal", mode: .terminal), pane("web")])
  #expect(ended == ["gone", "terminal"])
}

@Test func aSessionEndsWhenItsPaneSwitchesBetweenPrivateAndPersistent() {
  #expect(
    WebSessionLifecycle.ended(
      sessions: ["a": false, "b": true], panes: [pane("a", incognito: true), pane("b")])
      == ["a", "b"])
}

@Test func aWebPaneWithoutASessionEndsNothing() {
  #expect(WebSessionLifecycle.ended(sessions: [:], panes: [pane("a")]).isEmpty)
}

private let twoWorkspaces = [
  pane("a", workspace: "one"), pane("b", workspace: "two"),
  pane("t", mode: .terminal, workspace: "one"),
]

@Test func onlyTheWebPanesOfTheShownWorkspaceNeedASession() {
  #expect(
    WebSessionLifecycle.needed(
      panes: twoWorkspaces, shownWorkspaceId: "one", destination: .agents) == ["a"])
  #expect(
    WebSessionLifecycle.needed(
      panes: twoWorkspaces, shownWorkspaceId: "none", destination: .web
    ).isEmpty)
}

@Test func switchingTheShownWorkspaceMakesTheOtherPanesNeedASession() {
  let before = WebSessionLifecycle.needed(
    panes: twoWorkspaces, shownWorkspaceId: "one", destination: .web)
  let after = WebSessionLifecycle.needed(
    panes: twoWorkspaces, shownWorkspaceId: "two", destination: .web)
  #expect(before == ["a"])
  #expect(after == ["b"])
}

@Test func aPageThatDrawsNoWebPaneNeedsNoSession() {
  for destination in WorkspaceDestination.allCases where !destination.showsWebPanes {
    #expect(
      WebSessionLifecycle.needed(
        panes: twoWorkspaces, shownWorkspaceId: "one", destination: destination
      ).isEmpty, "\(destination)")
  }
  #expect(WorkspaceDestination.agents.showsWebPanes && WorkspaceDestination.web.showsWebPanes)
}
