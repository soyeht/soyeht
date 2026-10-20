import GameController
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The full-size surface for one pane: the writing workspace for writing panes, otherwise
/// the agent conversation, the terminal with input, or the web pane's "open on Mac" card.
struct PhonePaneDetail: View {
  let client: EngineClient
  let paneId: String

  var body: some View {
    if let pane = client.panes.first(where: { $0.paneId == paneId }), pane.mode == .writing {
      PhoneWritingWorkspace(
        client: client, workspaceId: pane.workspaceId, initialPaneId: pane.paneId)
    } else {
      PhonePane(client: client, paneId: paneId)
    }
  }
}

private struct PhonePane: View {
  let client: EngineClient
  let paneId: String
  @Environment(\.horizontalSizeClass) private var sizeClass
  private let fontStore = TerminalFontSizeStore.shared
  private let sizeModes = TerminalSizeModeStore.shared
  private let copyNotice = TerminalCopyNotice.shared

  /// A wide layout or an attached hardware keyboard means typing is the next thing to do,
  /// so the terminal takes the keyboard; on a bare iPhone a tap still brings it up.
  private var terminalClaimsKeyboard: Bool {
    sizeClass == .regular || GCKeyboard.coalesced != nil
  }

  var body: some View {
    VStack(spacing: 0) {
      if client.connected, let pane = client.panes.first(where: { $0.paneId == paneId }) {
        if pane.mode == .web {
          PhoneWebPane(client: client, pane: pane)
        } else if pane.mode == .agent {
          PhoneAgentTicker(client: client, pane: pane)
          AgentConversationView(
            client: client, paneId: paneId,
            harnessControl: { PhoneHarnessMenu(client: client, paneId: paneId) })
        } else {
          HStack {
            Text(pane.phrase)
            if pane.generation > 1 { Text("· new shell") }
            Spacer()
          }.font(.caption).padding(12).foregroundStyle(PhonePalette.secondary)
          PhoneTerminal(
            client: client, claimsKeyboardFocus: terminalClaimsKeyboard, fontStore: fontStore,
            sizeMode: sizeModes.mode(for: paneId))
          if let gap = client.gapText {
            Label(gap, systemImage: "exclamationmark.triangle").font(.caption).foregroundStyle(
              PhonePalette.warning
            ).padding(12)
          }
        }
      } else {
        ContentUnavailableView(
          "Updating screen", systemImage: "arrow.trianglehead.2.clockwise",
          description: Text(client.connectionText))
      }
    }.background(PhonePalette.face)
      .overlay(alignment: .bottom) {
        if copyNotice.visible { PhoneCopyNotice() }
      }
      .navigationTitle(client.panes.first(where: { $0.paneId == paneId })?.name ?? "Pane")
      .navigationBarTitleDisplayMode(.inline)
      .onAppear { client.select(paneId) }
      .toolbar {
        if client.panes.first(where: { $0.paneId == paneId })?.mode == .terminal {
          ToolbarItem(placement: .primaryAction) {
            PhoneTerminalMenu(
              client: client, paneId: paneId, fontStore: fontStore, sizeModes: sizeModes)
          }
        }
      }
  }
}

/// The terminal's text size and which screen decides the pane's size.
struct PhoneTerminalMenu: View {
  let client: EngineClient
  let paneId: String
  let fontStore: TerminalFontSizeStore
  let sizeModes: TerminalSizeModeStore

  var body: some View {
    let mode = sizeModes.mode(for: paneId)
    Menu {
      Section("Text size \(Int(fontStore.size)) pt") {
        Button("Larger", systemImage: "plus.magnifyingglass") { fontStore.apply(.larger) }
          .disabled(!fontStore.canGrow)
        Button("Smaller", systemImage: "minus.magnifyingglass") { fontStore.apply(.smaller) }
          .disabled(!fontStore.canShrink)
        Button("Default size", systemImage: "arrow.counterclockwise") {
          fontStore.apply(.reset)
        }
      }
      Section("Pane size") {
        Button(
          "Use the Mac's size", systemImage: mode == .followMac ? "checkmark" : "macbook"
        ) { sizeModes.choose(.followMac, for: paneId, client: client) }
        Button(
          "Fit to this screen", systemImage: mode == .fitThisScreen ? "checkmark" : "ipad"
        ) { sizeModes.choose(.fitThisScreen, for: paneId, client: client) }
      }
    } label: {
      Label("Terminal", systemImage: "textformat.size")
    }
    .accessibilityIdentifier("terminal-menu")
  }
}

/// The agent pane's "what is happening" line under the navigation title, as in the Mac
/// pane header: the rolling ticker and the status word, `accent` when the agent needs you.
struct PhoneAgentTicker: View {
  let client: EngineClient
  let pane: Pane

  var body: some View {
    let needsYou = AgentHeaderStatus.needsYou(
      state: pane.state, view: client.agentPanes[pane.paneId] ?? AgentPaneView())
    HStack(spacing: PaneInteriorMetrics.compactSpacing) {
      AgentPaneTickerView(
        client: client, paneId: pane.paneId, font: .caption.weight(.medium),
        timeFont: .caption2.monospaced().weight(.medium), color: PhonePalette.secondary)
      Text(
        AgentPaneStatusPresentation.title(
          state: pane.state, needsAuthentication: pane.agent?.authMethods.isEmpty == false,
          hasSession: pane.agent?.sessionId != nil)
      )
      .font(.caption2.weight(.semibold))
      .foregroundStyle(needsYou ? PhonePalette.accent : PhonePalette.secondary)
      .lineLimit(1).fixedSize()
    }
    .padding(.horizontal, PaneInteriorMetrics.phoneInset)
    .padding(.vertical, PaneInteriorMetrics.tightSpacing)
  }
}

struct PhoneWebPane: View {
  let client: EngineClient
  let pane: Pane

  var body: some View {
    VStack(spacing: 20) {
      Spacer()
      Image(systemName: "macbook.and.iphone")
        .font(.system(size: 42, weight: .light))
        .foregroundStyle(PhonePalette.accent)
      VStack(spacing: 7) {
        Text(pane.title?.isEmpty == false ? pane.title ?? pane.name : pane.name)
          .font(.system(.title3, design: .rounded, weight: .bold))
          .multilineTextAlignment(.center)
        Text(pane.url ?? "No address")
          .font(.system(.footnote, design: .monospaced))
          .foregroundStyle(PhonePalette.secondary)
          .multilineTextAlignment(.center)
          .textSelection(.enabled)
      }
      Button {
        openOnMac()
      } label: {
        Label("Open on the Mac", systemImage: "macbook")
          .font(.system(.headline, design: .rounded))
          .frame(maxWidth: 260)
      }
      .buttonStyle(.borderedProminent)
      Text("The page stays on your Mac. This device shows its title and address only.")
        .font(.footnote)
        .foregroundStyle(PhonePalette.secondary)
        .multilineTextAlignment(.center)
        .frame(maxWidth: 300)
      Spacer()
    }
    .padding(28)
  }

  private func openOnMac() {
    guard let url = pane.url else { return }
    Task {
      do {
        _ = try await client.navigateWeb(paneId: pane.paneId, url: url)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

/// Shown for a moment when a program in the terminal wrote to the clipboard.
struct PhoneCopyNotice: View {
  var body: some View {
    Label(TerminalCopyNotice.text, systemImage: "doc.on.clipboard")
      .font(.caption.weight(.semibold))
      .foregroundStyle(PhonePalette.ink)
      .padding(PaneInteriorMetrics.phoneInset)
      .background(PhonePalette.face, in: Capsule())
      .padding(PaneInteriorMetrics.phoneInset)
      .accessibilityAddTraits(.isStaticText)
      .transition(.opacity)
  }
}
