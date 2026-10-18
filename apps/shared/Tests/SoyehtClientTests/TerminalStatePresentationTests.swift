import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func onlyRealTerminalProblemsReceiveWarningBanners() {
  for state in [
    PaneState.starting, .working, .idle, .waiting_for_you, .cancelling, .recovering, .unknown,
  ] {
    #expect(TerminalStatePresentation.notice(for: state, exitCode: nil) == nil)
  }

  let signalLoss = TerminalStatePresentation.notice(for: .no_signal, exitCode: nil)
  #expect(signalLoss?.severity == .warning)
  #expect(signalLoss?.text.contains("stopped sending signals") == true)
  #expect(TerminalStatePresentation.recordedGap.severity == .warning)
}

@Test func aCleanExitNeverWarnsAndAFailedOneAlwaysDoes() {
  #expect(TerminalStatePresentation.notice(for: .dead, exitCode: 0)?.severity == .neutral)
  #expect(TerminalStatePresentation.notice(for: .dead, exitCode: nil)?.severity == .neutral)
  for failure in [1, 2, 130, -1] as [Int64] {
    #expect(
      TerminalStatePresentation.notice(for: .dead, exitCode: failure)?.severity == .warning,
      "exit \(failure)")
  }
}

@Test func theTerminalNoticeIsTheOneInTheVector() throws {
  let vector = try ContractVector.load("pane-state-presentation")
  for item in ContractVector.cases(vector, "terminalNotices") {
    let state = try #require(item["state"].string.flatMap(PaneState.init(rawValue:)))
    let notice = TerminalStatePresentation.notice(
      for: state, exitCode: item["exitCode"].int.map(Int64.init))
    #expect(notice?.text == item["text"].string, "\(item["name"].string ?? "")")
    let severity = notice.map { $0.severity == .warning ? "warning" : "neutral" }
    #expect(severity == item["severity"].string, "\(item["name"].string ?? "")")
  }
}
