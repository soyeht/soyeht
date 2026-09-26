import Foundation
import SoyehtProtocol

extension EngineClient {
  public func paneDisplayName(for identity: String) -> String {
    PaneDisplayName.resolve(identity, panes: panes)
  }
}

enum PaneDisplayName {
  static func resolve(_ identity: String, panes: [Pane]) -> String {
    panes.first(where: { $0.paneId == identity })?.name ?? identity
  }
}
