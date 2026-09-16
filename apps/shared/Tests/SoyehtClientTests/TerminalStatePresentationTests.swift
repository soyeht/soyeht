import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func terminalStatesArePresentedInWords() {
  let expected: [(PaneState, String)] = [
    (.starting, "Terminal is starting."),
    (.working, "Terminal is working."),
    (.idle, "Terminal is idle."),
    (.waiting_for_you, "Terminal is waiting for your input."),
    (.cancelling, "Terminal work is being cancelled."),
    (
      .no_signal,
      "The terminal has stopped sending signals. You can keep waiting or press Control-C."
    ),
    (.recovering, "Terminal is being restored."),
    (.dead, "Terminal process exited."),
    (
      .unknown,
      "Terminal state is unknown. Soyeht cannot tell whether work is still running. You can keep using the terminal."
    ),
  ]

  for (state, text) in expected {
    #expect(TerminalStatePresentation.text(for: state) == text)
  }
}

@Test func unknownTerminalStateNamesUncertaintyAndAUsableNextStep() {
  let text = TerminalStatePresentation.text(for: .unknown)
  #expect(text.contains("unknown"))
  #expect(text.contains("cannot tell"))
  #expect(text.contains("keep using"))
}
