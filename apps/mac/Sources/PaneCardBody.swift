import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// What a pane shows under its header: the web page, the agent conversation, or the terminal.
struct PaneCardBody: View {
  @Bindable var client: EngineClient
  @Environment(\.terminalKeyboardInputEnabled) private var terminalKeyboardInputEnabled
  let webHost: MacWebHost
  let pane: Pane
  let isVisible: Bool

  var body: some View {
    if pane.mode == .web {
      WebPaneSurface(client: client, host: webHost, pane: pane)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    } else if pane.mode == .agent {
      agentBody
    } else {
      terminalBody
    }
  }

  private var agentBody: some View {
    VStack(spacing: MacTheme.Space.x12) {
      if let task = client.tasks.first(where: { $0.ownerPaneId == pane.paneId }) {
        HStack(spacing: MacTheme.Space.x8) {
          Image(systemName: "list.bullet.clipboard").foregroundStyle(MacTheme.accent)
          Text(task.taskId).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
            .foregroundStyle(MacTheme.muted)
          Text(task.title).font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
            .lineLimit(1)
          Spacer()
          if let branch = task.worktree?.branch {
            Label(branch, systemImage: "arrow.triangle.branch")
              .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
              .foregroundStyle(MacTheme.muted)
          }
          Image(systemName: "chevron.right").foregroundStyle(MacTheme.muted)
        }
        .padding(.horizontal, MacTheme.Space.x12)
        .frame(height: PaneLayoutContentMetrics.agentTaskHeight)
        .paneCard()
      }
      AgentConversationView(
        client: client, paneId: pane.paneId, isVisible: isVisible,
        showsConfiguration: false,
        style: MacTheme.agentConversation,
        harnessControl: {
          if let agent = pane.agent {
            PaneHarnessControl(client: client, pane: pane, agent: agent)
          }
        })
    }.padding(
      EdgeInsets(
        top: PaneLayoutContentMetrics.agentBodyTopInset, leading: 4, bottom: 0,
        trailing: 4))
  }

  private var terminalBody: some View {
    VStack(spacing: 0) {
      if let notice = TerminalStatePresentation.notice(for: pane.state, exitCode: pane.exitCode) {
        Label(notice.text, systemImage: notice.systemImage)
          .font(MacTheme.ui(MacTheme.TypeScale.control, weight: .semibold))
          .foregroundStyle(noticeColor(notice))
          .frame(maxWidth: .infinity, alignment: .leading)
          .padding(.horizontal, MacTheme.Space.x10)
          .padding(.vertical, PaneLayoutContentMetrics.terminalStateVerticalInset)
          .background(noticeBackground(notice))
          .accessibilityIdentifier("terminal-state-\(pane.paneId)")
      }
      MacTerminal(client: client, paneId: pane.paneId)
        .environment(
          \.terminalKeyboardInputEnabled,
          terminalKeyboardInputEnabled && client.selectedPaneId == pane.paneId)
      if let gap = client.gapText(for: pane.paneId) {
        Label(gap, systemImage: TerminalStatePresentation.recordedGap.systemImage)
          .font(MacTheme.ui(MacTheme.TypeScale.control))
          .foregroundStyle(noticeColor(TerminalStatePresentation.recordedGap))
          .padding(MacTheme.Space.x10)
      }
    }
  }

  private func noticeColor(_ notice: TerminalNotice) -> Color {
    switch notice.severity {
    case .neutral: MacTheme.secondary
    case .warning: MacTheme.warning
    }
  }

  private func noticeBackground(_ notice: TerminalNotice) -> Color {
    switch notice.severity {
    case .neutral: MacTheme.face
    case .warning: MacTheme.well
    }
  }
}
