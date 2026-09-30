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
          AgentConversationView(client: client, paneId: paneId)
        } else {
          HStack {
            Text(pane.phrase)
            if pane.generation > 1 { Text("· new shell") }
            Spacer()
          }.font(.caption).padding(12).foregroundStyle(PhonePalette.secondary)
          PhoneTerminal(client: client, claimsKeyboardFocus: terminalClaimsKeyboard)
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
      .navigationTitle(client.panes.first(where: { $0.paneId == paneId })?.name ?? "Pane")
      .navigationBarTitleDisplayMode(.inline)
      .onAppear { client.select(paneId) }
  }
}

private struct PhoneWebPane: View {
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
