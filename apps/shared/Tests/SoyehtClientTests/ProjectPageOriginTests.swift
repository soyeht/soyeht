import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/project-page-origin.json`, which the engine passes as well.
@Test func projectPageOriginPassesTheSharedVectorCases() throws {
  let cases = ContractVector.cases(try ContractVector.load("project-page-origin"), "cases")
  #expect(cases.count > 20)
  for item in cases {
    let page = try #require(item["page"].string)
    let target = try #require(item["target"].string)
    let expected = try #require(item["same"].bool)
    #expect(
      ProjectPageOrigin.sameOrigin(page: page, target: target) == expected,
      "\(item["name"].string ?? "")")
  }
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
