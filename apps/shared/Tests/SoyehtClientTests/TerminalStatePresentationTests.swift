import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func onlyRealTerminalProblemsReceiveWarningBanners() {
  for state in [
    PaneState.starting, .working, .idle, .waiting_for_you, .cancelling, .recovering, .unknown,
  ] {
    #expect(TerminalStatePresentation.notice(for: state) == nil)
  }

  let exited = TerminalStatePresentation.notice(for: .dead)
  #expect(exited?.severity == .neutral)
  #expect(exited?.text == "Terminal process exited.")

  let signalLoss = TerminalStatePresentation.notice(for: .no_signal)
  #expect(signalLoss?.severity == .warning)
  #expect(signalLoss?.text.contains("stopped sending signals") == true)
  #expect(TerminalStatePresentation.recordedGap.severity == .warning)
}
