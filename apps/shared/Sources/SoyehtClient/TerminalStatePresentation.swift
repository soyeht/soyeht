import SoyehtProtocol

public struct TerminalNotice: Equatable, Sendable {
  public enum Severity: Equatable, Sendable {
    case neutral
    case warning
  }

  public let text: String
  public let systemImage: String
  public let severity: Severity

  public init(text: String, systemImage: String, severity: Severity) {
    self.text = text
    self.systemImage = systemImage
    self.severity = severity
  }
}

public enum TerminalStatePresentation {
  /// Normal activity and an unclassified shell do not warrant an alert banner.
  /// A process exit is informational because Pane does not carry its exit code.
  public static func notice(for state: PaneState) -> TerminalNotice? {
    switch state {
    case .dead:
      TerminalNotice(
        text: "Terminal process exited.", systemImage: "stop.circle",
        severity: .neutral)
    case .no_signal:
      TerminalNotice(
        text: "The terminal has stopped sending signals. You can keep waiting or press Control-C.",
        systemImage: "exclamationmark.triangle.fill", severity: .warning)
    case .starting, .working, .idle, .waiting_for_you, .cancelling, .recovering, .unknown:
      nil
    }
  }

  public static let recordedGap = TerminalNotice(
    text: "Some output was not recorded. The screen contains a gap.",
    systemImage: "exclamationmark.triangle", severity: .warning)
}
