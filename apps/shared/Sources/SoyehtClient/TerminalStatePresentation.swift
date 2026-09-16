import SoyehtProtocol

public enum TerminalStatePresentation {
  public static func text(for state: PaneState) -> String {
    switch state {
    case .starting:
      "Terminal is starting."
    case .working:
      "Terminal is working."
    case .idle:
      "Terminal is idle."
    case .waiting_for_you:
      "Terminal is waiting for your input."
    case .cancelling:
      "Terminal work is being cancelled."
    case .no_signal:
      "The terminal has stopped sending signals. You can keep waiting or press Control-C."
    case .recovering:
      "Terminal is being restored."
    case .dead:
      "Terminal process exited."
    case .unknown:
      "Terminal state is unknown. Soyeht cannot tell whether work is still running. You can keep using the terminal."
    }
  }
}
