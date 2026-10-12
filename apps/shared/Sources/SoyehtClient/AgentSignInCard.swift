import SoyehtProtocol
import SwiftUI

/// The sign-in card of an agent pane: the choices when the agent needs a sign-in, the progress of
/// one that runs, and the reason one did not finish. The engine owns every state.
struct AgentSignInCard: View {
  let client: EngineClient
  let paneId: String
  let state: AgentSignInState
  let style: AgentConversationStyle
  let inset: CGFloat

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.rowSpacing) {
      if let title = state.title {
        Label(title, systemImage: icon).font(style.headingFont).foregroundStyle(titleColor)
      }
      if let detail = state.detail {
        Text(detail).font(style.captionFont).foregroundStyle(style.secondary)
      }
      content
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .padding(PaneInteriorMetrics.cardPadding)
    .paneInteriorFill(style.canvas)
    .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("agent-sign-in-\(paneId)")
  }

  private var icon: String {
    switch state {
    case .failed: "exclamationmark.triangle"
    case .waiting: "safari"
    case .verifying, .signingOut: "clock"
    case .hidden, .choose: "person.badge.key"
    }
  }

  private var titleColor: Color {
    switch state {
    case .failed, .choose: style.warning
    case .hidden, .waiting, .verifying, .signingOut: style.ink
    }
  }

  /// Progress in palette roles: the system spinner ignores the palette.
  private var progress: some View {
    SignInProgressBar(track: style.well, fill: style.ink)
      .accessibilityLabel("In progress")
  }

  @ViewBuilder private var content: some View {
    switch state {
    case .hidden:
      EmptyView()
    case .choose(let methods, let failure):
      choose(methods, failure: failure)
    case .waiting(let terminalPaneId):
      HStack(spacing: PaneInteriorMetrics.stackSpacing) {
        progress
        Spacer(minLength: 0)
        #if os(macOS)
          if let terminalPaneId {
            Button(AgentSignInLabels.showTerminal) { client.select(terminalPaneId) }
              .buttonStyle(PanePillButtonStyle(.secondary, style: style))
          }
        #endif
        Button(AgentSignInLabels.cancelSignIn, role: .destructive) {
          client.cancelAgentAuthentication(paneId)
        }
        .buttonStyle(PanePillButtonStyle(.destructive, style: style))
      }
    case .verifying, .signingOut:
      progress
    case .failed:
      HStack {
        Spacer()
        Button(AgentSignInLabels.dismiss) { client.dismissSignInFailure(paneId) }
          .buttonStyle(PanePillButtonStyle(.secondary, style: style))
      }
    }
  }

  @ViewBuilder private func choose(_ methods: [AgentSignInMethod], failure: String?) -> some View {
    if let failure {
      Text(failure).font(style.captionFont).foregroundStyle(style.warning)
    }
    #if os(macOS)
      if methods.contains(where: \.opensTerminal) {
        Text(AgentSignInLabels.chooseTerminalHint)
          .font(style.captionFont).foregroundStyle(style.secondary)
      }
      ForEach(Array(methods.enumerated()), id: \.element.id) { index, method in
        VStack(alignment: .leading, spacing: PaneInteriorMetrics.tightSpacing) {
          Button {
            client.startAgentAuthentication(paneId, methodId: method.id)
          } label: {
            Label(
              method.name, systemImage: method.opensTerminal ? "terminal" : "arrow.right.circle"
            )
            .frame(maxWidth: .infinity)
          }
          .buttonStyle(PanePillButtonStyle(index == 0 ? .primary : .secondary, style: style))
          if let detail = method.detail {
            Text(detail).font(style.captionFont).foregroundStyle(style.secondary)
          }
        }
      }
    #else
      Text("Finish sign-in on the Mac, then return here.")
        .font(style.captionFont).foregroundStyle(style.secondary)
    #endif
  }
}

/// An indeterminate bar: a segment of `fill` runs along a `track`.
private struct SignInProgressBar: View {
  let track: Color
  let fill: Color

  var body: some View {
    TimelineView(.animation) { timeline in
      let phase =
        timeline.date.timeIntervalSinceReferenceDate
        .truncatingRemainder(dividingBy: 1.4) / 1.4
      GeometryReader { proxy in
        let width = proxy.size.width
        ZStack(alignment: .leading) {
          Capsule().fill(track)
          Capsule().fill(fill).frame(width: width * 0.3)
            .offset(x: width * 1.3 * phase - width * 0.3)
        }
        .clipShape(Capsule())
      }
    }
    .frame(height: PaneInteriorMetrics.tightSpacing)
  }
}
