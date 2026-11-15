import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/project-page-origin.json`, which the engine passes as well: the
/// origin rule, and the navigation decision of a main frame, which is the same rule.
@Test func projectPageOriginPassesTheSharedVectorCases() throws {
  let cases = ContractVector.cases(try ContractVector.load("project-page-origin"), "cases")
  #expect(cases.count > 20)
  for item in cases {
    let page = try #require(item["page"].string)
    let target = try #require(item["target"].string)
    let expected = try #require(item["same"].bool)
    let name = "\(item["name"].string ?? "")"
    #expect(ProjectPageOrigin.sameOrigin(page: page, target: target) == expected, "\(name)")
    #expect(
      ProjectPageNavigation.allows(
        isProjectPage: true, pageAddress: page, target: target, isMainFrame: true,
        hasTargetFrame: true) == expected,
      "\(name)")
  }
}

@Test func aSubFrameOfAProjectPageMayLoadWhatThePageMakesItself() {
  let page = "http://127.0.0.1:5000/0123/index.html"
  func frame(_ target: String, main: Bool) -> Bool {
    ProjectPageNavigation.allows(
      isProjectPage: true, pageAddress: page, target: target, isMainFrame: main,
      hasTargetFrame: true)
  }
  for target in ["about:blank", "about:srcdoc", "data:text/html,x", "blob:http://127.0.0.1:5000/1"]
  {
    #expect(frame(target, main: false), "\(target)")
    #expect(!frame(target, main: true), "\(target)")
  }
  #expect(frame("http://127.0.0.1:5000/0123/b.html", main: false))
  #expect(!frame("http://example.com/", main: false))
}

@Test func aProjectPageWithNoAddressYetLoadsNothing() {
  for target in [
    "http://127.0.0.1:5000/0123/index.html", "about:blank", "http://example.com/", "",
  ] {
    for main in [true, false] {
      #expect(
        !ProjectPageNavigation.allows(
          isProjectPage: true, pageAddress: nil, target: target, isMainFrame: main,
          hasTargetFrame: true),
        "\(target) main=\(main)")
    }
  }
}

@Test func aProjectPageNeverOpensANewWindow() {
  let page = "http://127.0.0.1:5000/0123/index.html"
  for target in [page, "about:blank", "http://example.com/"] {
    #expect(
      !ProjectPageNavigation.allows(
        isProjectPage: true, pageAddress: page, target: target, isMainFrame: false,
        hasTargetFrame: false),
      "\(target)")
  }
}

@Test func aPaneThatIsNotAProjectPageIsLeftToItsOwnSchemePolicy() {
  for address in [nil, "http://127.0.0.1:5000/a"] {
    for hasFrame in [true, false] {
      #expect(
        ProjectPageNavigation.allows(
          isProjectPage: false, pageAddress: address, target: "http://example.com/",
          isMainFrame: true, hasTargetFrame: hasFrame))
    }
  }
}

private func command(_ kind: WebCommandKind, _ args: [String: JsonValue]) -> WebCommandParams {
  WebCommandParams(
    paneId: "p", command: kind, args: .object(args), timeoutMs: 1000, operationKey: "k")
}

private func access(_ session: String, background: Bool? = nil) -> JsonValue {
  var values: [String: JsonValue] = ["session": .string(session)]
  if let background { values["background"] = .bool(background) }
  return .object(values)
}

@Test func onlyABackgroundReloadIsNotAPersonsCommand() {
  func human(_ kind: WebCommandKind, action: String?, background: Bool?) -> Bool {
    var args: [String: JsonValue] = ["_soyehtAccess": access("human", background: background)]
    if let action { args["action"] = .string(action) }
    return WebCommandAccess.isPersons(command(kind, args))
  }
  #expect(!human(.navigate, action: "reload", background: true))
  #expect(human(.navigate, action: "reload", background: nil))
  #expect(human(.navigate, action: "reload", background: false))
  // The flag on anything else is ignored: a click, or a navigate that is not a reload.
  #expect(human(.click, action: nil, background: true))
  #expect(human(.navigate, action: "back", background: true))
  #expect(human(.navigate, action: nil, background: true))
}

@Test func aCommandOfAnotherSessionOrWithNoAccessIsNotAPersonsCommand() {
  #expect(!WebCommandAccess.isPersons(command(.click, ["_soyehtAccess": access("granted")])))
  #expect(!WebCommandAccess.isPersons(command(.click, ["_soyehtAccess": access("probe")])))
  #expect(!WebCommandAccess.isPersons(command(.click, [:])))
  #expect(
    !WebCommandAccess.isPersons(
      WebCommandParams(
        paneId: "p", command: .click, args: .null, timeoutMs: 1000, operationKey: "k")))
}

@Test func aFrameOfAProjectPageMayAlsoLoadWhatThePageMakesItself() {
  let page = "http://127.0.0.1:5000/0123/index.html"
  for target in ["about:blank", "about:srcdoc", "data:text/html,x", "blob:http://127.0.0.1:5000/1"]
  {
    #expect(ProjectPageOrigin.allowsFrame(page: page, target: target), "\(target)")
    #expect(!ProjectPageOrigin.sameOrigin(page: page, target: target), "\(target)")
  }
  #expect(ProjectPageOrigin.allowsFrame(page: page, target: "http://127.0.0.1:5000/0123/b.html"))
  for target in ["http://example.com/", "file:///etc/passwd", "javascript:alert(1)", ""] {
    #expect(!ProjectPageOrigin.allowsFrame(page: page, target: target), "\(target)")
  }
}

@Test func aProjectPageIsCaptionedByItsPathNeverByItsAddress() {
  var pane = PaneLayoutFixture.pane("web", workspace: "work", mode: .web)
  pane.url = "http://127.0.0.1:5000/secret-token/index.html"
  #expect(WebPaneCaption.text(for: pane) == "http://127.0.0.1:5000/secret-token/index.html")
  pane.projectPath = "site/index.html"
  #expect(WebPaneCaption.text(for: pane) == "site/index.html")
  #expect(WebPaneCaption.explanation(for: pane).contains("title and file only"))
  pane.url = nil
  pane.projectPath = nil
  #expect(WebPaneCaption.text(for: pane) == WebPaneCaption.noAddress)
}
