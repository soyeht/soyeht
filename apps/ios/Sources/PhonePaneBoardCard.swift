import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// One pane of the board: a shell holding the pane's header over its body. The shell is the only
/// raised surface; everything inside is flat. The focused pane has an `accent` outline.
struct PhonePaneBoardCard: View {
  let client: EngineClient
  let pane: Pane
  let isFocused: Bool
  let colors: SoyehtThemeColorSet
  let onFocus: () -> Void
  private let fontStore = TerminalFontSizeStore.shared
  private let sizeModes = TerminalSizeModeStore.shared

  var body: some View {
    VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
      header
      bodyContent
    }
    .padding(PaneLayoutContentMetrics.paneCardInset)
    .paneShell(colors)
    .overlay {
      RoundedRectangle(cornerRadius: PaneShellMetrics.cornerRadius, style: .continuous)
        .strokeBorder(
          isFocused ? PhonePalette.accent : Color.clear,
          lineWidth: PaneInteriorMetrics.separatorWidth)
    }
    .contentShape(Rectangle())
    .simultaneousGesture(TapGesture().onEnded(onFocus))
    .contextMenu {
      Button("Close pane", role: .destructive) { client.closePane(pane.paneId) }
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel(pane.name)
    .accessibilityAddTraits(isFocused ? .isSelected : [])
  }

  private var header: some View {
    HStack(spacing: PaneInteriorMetrics.rowSpacing) {
      PaneStateDot(pane.state)
      Text(pane.name)
        .font(PaneInteriorFont.ui(PaneInteriorMetrics.bodyPointSize, weight: .heavy))
        .foregroundStyle(PhonePalette.ink)
        .lineLimit(1)
      Spacer(minLength: 0)
      if pane.mode == .terminal {
        PhoneTerminalMenu(
          client: client, paneId: pane.paneId, fontStore: fontStore, sizeModes: sizeModes)
      }
    }
    .padding(.horizontal, PaneInteriorMetrics.cardPadding)
    .frame(height: PaneLayoutContentMetrics.genericPaneHeaderHeight)
    .paneInteriorSeparator(SoyehtThemeColors.border)
  }

  @ViewBuilder private var bodyContent: some View {
    switch pane.mode {
    case .agent:
      PhoneAgentTicker(client: client, pane: pane)
      AgentConversationView(
        client: client, paneId: pane.paneId,
        harnessControl: { PhoneHarnessMenu(client: client, paneId: pane.paneId) })
    case .terminal:
      Text(pane.phrase)
        .font(.caption).foregroundStyle(PhonePalette.secondary)
        .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, PaneInteriorMetrics.cardPadding)
      // Each terminal obeys its pane's size mode: following the Mac it reports nothing, so the
      // Mac's terminal is not reflowed; fitted it reports the size of its cell on the board.
      PhoneTerminal(
        client: client, paneId: pane.paneId, claimsKeyboardFocus: isFocused,
        fontStore: fontStore, sizeMode: sizeModes.mode(for: pane.paneId), onActivate: onFocus)
      if let gap = client.gapText(for: pane.paneId) {
        Label(gap, systemImage: "exclamationmark.triangle")
          .font(.caption).foregroundStyle(PhonePalette.warning)
      }
    case .web:
      PhoneWebPane(client: client, pane: pane)
    case .editor, .writing:
      ContentUnavailableView(
        "Open on the Mac", systemImage: "macbook",
        description: Text("This pane is shown on the Mac for now."))
    }
  }
}
