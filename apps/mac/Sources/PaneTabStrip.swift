import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The tab strip over panes shown one at a time: an adaptive writing companion in tabs, or the
/// tab group `PaneLayoutFit` makes when the panes do not fit the window side by side.
struct PaneTabStrip: View {
  let panes: [Pane]
  let selectedPaneId: String?
  /// Prefix of the accessibility identifiers of the strip and of each tab.
  let identifier: String
  let onSelect: (String) -> Void

  var body: some View {
    ScrollView(.horizontal) {
      HStack(spacing: MacTheme.Space.x6) {
        ForEach(panes, id: \.paneId) { pane in
          Button {
            onSelect(pane.paneId)
          } label: {
            Label(pane.name, systemImage: icon(for: pane))
              .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
              .lineLimit(1)
              .padding(.horizontal, MacTheme.Space.x10)
              .frame(height: PaneLayoutContentMetrics.companionTabStripHeight - 8)
              .modifier(PaneTabSurface(selected: pane.paneId == selectedPaneId))
          }
          .buttonStyle(.plain)
          .accessibilityIdentifier("\(identifier)-\(pane.paneId)")
        }
      }
      .padding(.horizontal, MacTheme.Space.x4)
      .frame(height: PaneLayoutContentMetrics.companionTabStripHeight)
    }
    .scrollIndicators(.hidden)
    .background(MacTheme.canvas)
    .accessibilityIdentifier("\(identifier)s")
  }

  private func icon(for pane: Pane) -> String {
    switch pane.mode {
    case .agent: "cpu"
    case .terminal: "terminal"
    default: "doc.text"
    }
  }
}

private struct PaneTabSurface: ViewModifier {
  let selected: Bool

  func body(content: Content) -> some View {
    if selected {
      content.neoWell(radius: MacTheme.Radius.pill)
    } else {
      content.neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
    }
  }
}
