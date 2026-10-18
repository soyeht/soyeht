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
  /// Normal activity and an unclassified shell do not warrant an alert banner. A process exit
  /// warns only when its status says it failed or was interrupted: a one-shot command that
  /// finished cleanly, such as an agent sign-in, only informs. An exit with no recorded status
  /// (a pane the engine adopted after the fact) informs too.
  public static func notice(for state: PaneState, exitCode: Int64?) -> TerminalNotice? {
    switch state {
    case .dead:
      exitNotice(exitCode)
    case .no_signal:
      TerminalNotice(
        text: "The terminal has stopped sending signals. You can keep waiting or press Control-C.",
        systemImage: "exclamationmark.triangle.fill", severity: .warning)
    case .starting, .working, .idle, .waiting_for_you, .cancelling, .recovering, .unknown:
      nil
    }
  }

  private static func exitNotice(_ exitCode: Int64?) -> TerminalNotice {
    switch exitCode {
    case nil:
      TerminalNotice(
        text: "Terminal process exited.", systemImage: "stop.circle", severity: .neutral)
    case 0:
      TerminalNotice(
        text: "Terminal process finished.", systemImage: "stop.circle", severity: .neutral)
    case let code? where code > 0:
      TerminalNotice(
        text: "Terminal process exited with status \(code).",
        systemImage: "exclamationmark.triangle.fill", severity: .warning)
    default:
      TerminalNotice(
        text: "Terminal process ended without an exit status.",
        systemImage: "exclamationmark.triangle.fill", severity: .warning)
    }
  }

  public static let recordedGap = TerminalNotice(
    text: "Some output was not recorded. The screen contains a gap.",
    systemImage: "exclamationmark.triangle", severity: .warning)
}
