import SoyehtProtocol
import Testing
@testable import SoyehtClient

@Test func timeoutDiagnosticIncludesMethodDurationAndPaneIdWithoutPayload() {
  let params: JsonValue = .object([
    "paneId": .string("11111111-2222-4333-8444-555555555555"),
    "text": .string("private prompt"),
    "token": .string("private credential"),
  ])
  let diagnostic = EngineTimeoutDiagnostic(
    method: "pane.prompt", params: params, durationMs: 12_345)

  #expect(
    diagnostic.logLine
      == "method=pane.prompt durationMs=12345 paneId=11111111-2222-4333-8444-555555555555")
  #expect(!diagnostic.logLine.contains("private prompt"))
  #expect(!diagnostic.logLine.contains("private credential"))
}

@Test func timeoutDiagnosticNeverLogsAnUnvalidatedPaneId() {
  let diagnostic = EngineTimeoutDiagnostic(
    method: "pane.prompt", params: .object(["paneId": .string("private prompt")]),
    durationMs: 12_000)
  #expect(diagnostic.logLine == "method=pane.prompt durationMs=12000 paneId=none")
}

@Test func timeoutDiagnosticOmitsPaneIdWhenRequestHasNone() {
  let diagnostic = EngineTimeoutDiagnostic(
    method: "harness.list", params: .object(["rescan": .bool(true)]), durationMs: 12_000)
  #expect(diagnostic.logLine == "method=harness.list durationMs=12000 paneId=none")
}
