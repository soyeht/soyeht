#if os(iOS)
  import Foundation
  import UIKit

  /// Where a link tapped in a phone or iPad terminal opens. A page the device cannot reach
  /// (an address on the Mac itself) opens in a web pane on the Mac; any other address opens in
  /// the system browser, unless the person asked for web panes (`WebLinkPreferences`).
  @MainActor struct PhoneTerminalLinks {
    let client: EngineClient
    let preferences = WebLinkPreferences.shared

    func open(_ url: URL, workspaceId: String) {
      switch preferences.destination(for: url.absoluteString) {
      case .soyeht(let target):
        // The terminal stays the selected pane: the web pane is a page on the Mac, and
        // selecting it here would take the terminal's output away.
        Task {
          do {
            _ = try await client.openWeb(
              workspaceId: workspaceId, url: target.absoluteString, select: false)
          } catch { client.problem = error.localizedDescription }
        }
      case .system(let target):
        UIApplication.shared.open(target)
      case nil:
        client.problem = "Only HTTP and HTTPS links can be opened."
      }
    }
  }
#endif
