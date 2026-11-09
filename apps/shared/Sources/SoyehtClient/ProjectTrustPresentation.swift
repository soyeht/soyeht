import SoyehtProtocol

/// The words and the default of the "Trust this folder?" sheet and what the editor's status line
/// says about a restricted folder. Every sentence is a `ClientPolicy` text, so the Mac, the
/// iPhone and the desktop say the same.
public enum ProjectTrustPresentation {
  public struct Choice: Equatable, Sendable {
    public let choice: ProjectTrustChoice
    public let label: String
    /// The choice Return picks: the safe one.
    public let isDefault: Bool
  }

  public static let title = ClientPolicy.projectTrustTitle
  public static let summary = ClientPolicy.projectTrustSummary
  public static let restrictedNote = ClientPolicy.projectTrustRestrictedNote

  /// The two answers, the safe default first.
  public static let choices: [Choice] = [
    Choice(choice: .restricted, label: ClientPolicy.projectTrustRestrict, isDefault: true),
    Choice(choice: .trusted, label: ClientPolicy.projectTrustTrust, isDefault: false),
  ]

  /// The help line of the editor's language marker. A restricted server, or one running with
  /// code execution off, says why instead of printing the state alone.
  public static func languageServerHelp(_ status: LspServerStatus?) -> String {
    guard let status else { return "No language server is active" }
    var line = "Language server: \(status.state.rawValue)"
    if let detail = status.detail { line += " · \(detail)" }
    if status.state == .restricted || status.safeMode == true {
      line += ". \(restrictedNote)"
    }
    return line
  }
}
