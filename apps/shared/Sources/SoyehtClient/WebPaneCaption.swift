import SoyehtProtocol

/// What a client that does not render a web pane (the iPhone) shows under its title. A project
/// page's address carries the secret that serves it, so the person is shown the file that was
/// opened and never the address (contract 11, section 5b).
public enum WebPaneCaption {
  public static let noAddress = "No address"

  public static func text(for pane: Pane) -> String {
    if let path = pane.projectPath { return path }
    return pane.url ?? noAddress
  }

  /// The line under the caption: what the phone shows of a page that stays on the Mac.
  public static func explanation(for pane: Pane) -> String {
    let shown = pane.projectPath == nil ? "address" : "file"
    return "The page stays on your Mac. This device shows its title and \(shown) only."
  }
}
