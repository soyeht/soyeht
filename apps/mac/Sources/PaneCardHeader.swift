import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The header of a terminal, agent or web pane: its state dot, name, agent ticker and status,
/// and the actions `PaneHeaderControls` lists, inline or folded into one menu by the width.
struct PaneCardHeader: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let cardWidth: CGFloat
  let companionLinked: Bool
  let interaction: PaneLayoutInteractionState?
  let dragConfiguration: PaneHeaderDragConfiguration?
  let capture: PaneExperimentCapture
  @Binding var showingAutomation: Bool
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onClose: (String) -> Void

  var body: some View {
    let plan = MacPaneLayoutMetrics.paneHeader.plan(
      availableWidth: MacPaneLayoutMetrics.paneHeaderAvailableWidth(cardWidth: cardWidth),
      actionCount: actionCount,
      hasSummary: pane.mode == .agent, hasStatus: pane.mode == .agent)
    return content(plan: plan)
      .font(MacTheme.ui(MacTheme.TypeScale.control, weight: .medium))
      .foregroundStyle(MacTheme.secondary)
      .padding(.horizontal, MacTheme.Space.x12)
      .frame(height: PaneLayoutContentMetrics.genericPaneHeaderHeight)
      .contentShape(Rectangle())
      .paneInteriorSeparator(MacTheme.border)
      .paneHeaderDrag(dragConfiguration)
  }

  private func content(plan: PaneHeaderLayoutPlan) -> some View {
    HStack(spacing: MacTheme.Space.x7) {
      Circle().fill(pane.state.dot.color)
        .frame(
          width: PaneLayoutContentMetrics.genericPaneHeaderStateDotWidth,
          height: PaneLayoutContentMetrics.genericPaneHeaderStateDotWidth)
      Text(pane.name)
        .font(MacTheme.ui(MacTheme.TypeScale.body, weight: .heavy))
        .foregroundStyle(MacTheme.ink)
        .lineLimit(1).truncationMode(.tail).layoutPriority(3)
        .help(pane.name)
      if companionLinked {
        Image(systemName: "link")
          .help("Linked writing companion")
          .accessibilityLabel("Linked writing companion")
          .accessibilityIdentifier("writing-companion-link-\(pane.paneId)")
      }
      if pane.mode == .agent,
        plan.actionPlacement == .inline || plan.summaryMaximumWidth != nil
      {
        AgentPaneTickerView(
          client: client, paneId: pane.paneId,
          font: MacTheme.ui(MacTheme.TypeScale.control, weight: .medium),
          timeFont: MacTheme.mono(MacTheme.TypeScale.small, weight: .medium),
          color: MacTheme.secondary)
      } else {
        Spacer(minLength: MacTheme.Space.x2)
      }
      if pane.mode == .agent {
        Text(statusText)
          .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(needsYou ? MacTheme.accent : MacTheme.secondary)
          .lineLimit(1).truncationMode(.tail)
          .frame(maxWidth: MacPaneLayoutMetrics.paneHeader.metrics.statusWidth)
          .fixedSize().layoutPriority(2)
      }
      if plan.actionPlacement == .inline {
        inlineActions
      } else {
        compactActions
      }
    }
  }

  /// The agent asks for the person: the header's status word turns `accent`.
  private var needsYou: Bool {
    AgentHeaderStatus.needsYou(
      state: pane.state, view: client.agentPanes[pane.paneId] ?? AgentPaneView())
  }

  @ViewBuilder private var inlineActions: some View {
    #if DEBUG
      captureFeedback
      if recordsExperiments { captureButton }
    #endif
    ForEach(controls.actions, id: \.self) { action in
      Button {
        perform(action)
      } label: {
        Image(systemName: action.systemImage)
      }
      .help(action.title)
      .accessibilityLabel(action.title)
      .accessibilityIdentifier(action.accessibilityIdentifier(paneId: pane.paneId))
      .disabled(isDisabled(action))
    }
  }

  private var compactActions: some View {
    Menu {
      #if DEBUG
        if recordsExperiments { captureMenuButton }
      #endif
      ForEach(controls.actions, id: \.self) { action in
        if action == .splitColumns || action == .close { Divider() }
        Button(role: action == .close ? .destructive : nil) {
          perform(action)
        } label: {
          Label(action.title, systemImage: action.systemImage)
        }
        .disabled(isDisabled(action))
      }
    } label: {
      Image(systemName: "ellipsis.circle")
        .frame(width: PaneLayoutContentMetrics.genericPaneHeaderActionWidth)
    }
    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
    .help("Pane actions")
    .accessibilityLabel("Pane actions")
    .accessibilityIdentifier("pane-header-actions-\(pane.paneId)")
  }

  /// The icon actions this header shows (`PaneHeaderControls`).
  private var controls: PaneHeaderControls { PaneHeaderControls(mode: pane.mode) }

  private func perform(_ action: PaneHeaderAction) {
    switch action {
    case .loop: showingAutomation.toggle()
    case .tasks: onTasks()
    case .splitColumns: onSplit(pane.paneId, .columns)
    case .splitRows: onSplit(pane.paneId, .rows)
    case .close: onClose(pane.paneId)
    }
  }

  private func isDisabled(_ action: PaneHeaderAction) -> Bool {
    switch action {
    case .loop, .tasks: false
    case .splitColumns, .splitRows, .close:
      !client.connected || interaction?.blocksLayoutActions == true
    }
  }

  private var statusText: String {
    guard pane.mode == .agent else {
      return pane.state.rawValue.replacingOccurrences(of: "_", with: " ")
    }
    return AgentPaneStatusPresentation.title(
      state: pane.state, needsAuthentication: pane.agent?.authMethods.isEmpty == false,
      hasSession: pane.agent?.sessionId != nil)
  }

  private var recordsExperiments: Bool { [.terminal, .agent].contains(pane.mode) }

  private var actionCount: Int {
    var count = controls.actions.count
    #if DEBUG
      if recordsExperiments { count += 1 }
    #endif
    return count
  }

  #if DEBUG
    @ViewBuilder private var captureFeedback: some View {
      if recordsExperiments, let experiment = pane.experimentCapture {
        if experiment.on {
          Label(
            ExperimentCapturePresentation.activeLabel(for: pane.mode),
            systemImage: "record.circle.fill"
          )
          .foregroundStyle(MacTheme.danger).lineLimit(1)
          .accessibilityIdentifier("experiment-recording-\(pane.paneId)")
        } else if let failure = experiment.failure {
          Label("Capture stopped", systemImage: "exclamationmark.triangle.fill")
            .foregroundStyle(MacTheme.warning).lineLimit(1)
            .help(failure)
            .accessibilityLabel("Capture stopped: \(failure)")
        } else if let notice = capture.notice {
          Text(notice).foregroundStyle(MacTheme.secondary).lineLimit(1)
        }
      }
    }

    private var captureButton: some View {
      Button {
        capture.toggle(pane, client: client)
      } label: {
        Image(systemName: captureSymbol)
      }
      .help(captureActionTitle)
      .accessibilityLabel(captureActionTitle)
      .accessibilityIdentifier("experiment-capture-\(pane.paneId)")
      .disabled(!client.connected || capture.inFlight)
    }

    private var captureMenuButton: some View {
      Button {
        capture.toggle(pane, client: client)
      } label: {
        Label(captureActionTitle, systemImage: captureSymbol)
      }
      .disabled(!client.connected || capture.inFlight)
    }

    private var captureSymbol: String {
      pane.experimentCapture?.on == true ? "stop.circle" : "record.circle"
    }

    private var captureActionTitle: String {
      pane.experimentCapture?.on == true
        ? "Stop experiment recording" : "Start experiment recording"
    }
  #endif
}
