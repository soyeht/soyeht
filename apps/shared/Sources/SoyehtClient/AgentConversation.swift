import Foundation
import SoyehtProtocol
import SwiftUI

#if os(macOS)
  import AppKit
#endif

extension AgentMessageSource {
  /// Who sent a peer message, by name; the pane identifier only routes replies.
  var displayName: String {
    name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? harness : name
  }
}

struct AgentSilencePresentation: Sendable {
  let elapsedSeconds: Int?
  private let paneState: PaneState?

  init(silentSince: String?, paneState: PaneState? = nil, now: Date = Date()) {
    self.paneState = paneState
    let interval = silentSince.flatMap(UsageTimestamp.date(from:))
      .map { now.timeIntervalSince($0) }
    elapsedSeconds = interval.flatMap { value in
      value.isFinite && value >= 0 ? Int(value) : nil
    }
  }

  var escalated: Bool {
    paneState == .no_signal
      || elapsedSeconds.map { $0 >= ProtocolLimits.noSignalSec } == true
  }

  var elapsed: String? { elapsedSeconds.map(AgentElapsedText.text(seconds:)) }

  var title: String {
    switch (escalated, elapsed) {
    case (true, let elapsed?): "No output from the agent for \(elapsed)"
    case (true, nil): "The agent has produced no output"
    case (false, let elapsed?): "Waiting for the first agent output · \(elapsed)"
    case (false, nil): "Waiting for the first agent output"
    }
  }

  var detail: String {
    if elapsedSeconds == nil {
      return
        "The turn is active. Elapsed time is unavailable. You can keep waiting or cancel this turn."
    }
    return escalated
      ? "The turn is still active. You can keep waiting or cancel this turn."
      : "The turn is active. Nothing from the agent has arrived yet."
  }

  static let cancelTurnTitle = "Cancel turn"
}

enum AgentElapsedText {
  static func text(seconds elapsed: Int) -> String {
    let hours = elapsed / 3_600
    let minutes = (elapsed % 3_600) / 60
    let seconds = elapsed % 60
    if hours > 0 { return "\(hours)h \(minutes)m" }
    return "\(minutes)m \(seconds)s"
  }
}

extension AgentConversationView where HarnessControl == EmptyView {
  public init(
    client: EngineClient, paneId: String, isVisible: Bool = true,
    showsConfiguration: Bool = true,
    style: AgentConversationStyle = .standard,
    onExplicitRemoteAction: @escaping () -> Void = {}
  ) {
    self.init(
      client: client, paneId: paneId, isVisible: isVisible,
      showsConfiguration: showsConfiguration, style: style,
      onExplicitRemoteAction: onExplicitRemoteAction, harnessControl: { EmptyView() })
  }
}

/// `HarnessControl` is the composer's harness chip after "+": each host wraps
/// `AgentHarnessChipLabel` in its own harness switch (a popover on the Mac, a menu on the
/// iPhone and iPad).
@MainActor public struct AgentConversationView<HarnessControl: View>: View {
  @Bindable private var client: EngineClient
  private let paneId: String
  private let isVisible: Bool
  private let showsConfiguration: Bool
  private let style: AgentConversationStyle
  private let onExplicitRemoteAction: () -> Void
  private let harnessControl: HarnessControl
  @State private var viewer = AgentPaneViewer()
  @State private var follow = AgentConversationFollow()
  @State private var bottomVisibleId: String?
  @State private var subagents = AgentSubagentDisclosure()
  @FocusState private var promptFocused: Bool
  @State private var placeCaretAtEnd = false

  public init(
    client: EngineClient, paneId: String, isVisible: Bool = true,
    showsConfiguration: Bool = true,
    style: AgentConversationStyle = .standard,
    onExplicitRemoteAction: @escaping () -> Void = {},
    @ViewBuilder harnessControl: () -> HarnessControl
  ) {
    self.client = client
    self.paneId = paneId
    self.isVisible = isVisible
    self.showsConfiguration = showsConfiguration
    self.style = style
    self.onExplicitRemoteAction = onExplicitRemoteAction
    self.harnessControl = harnessControl()
  }

  private var pane: Pane? { client.pane(paneId) }
  /// What is typed and not sent; it outlives this view (see `AgentPromptDrafts`).
  private var prompt: String {
    get { AgentPromptDrafts.shared.text(for: paneId) }
    nonmutating set { AgentPromptDrafts.shared.set(newValue, for: paneId) }
  }
  /// The files attached and not sent; they outlive this view with the typed text.
  private var attachments: AgentComposerAttachments {
    AgentPromptDrafts.shared.attachments(for: paneId, linkHostFiles: !client.pairedDevice)
  }
  private var promptBinding: Binding<String> { Binding(get: { prompt }, set: { prompt = $0 }) }
  private var view: AgentPaneView { client.agentPanes[paneId] ?? AgentPaneView() }
  /// The transcript grouped once per change of the pane, not once per evaluation of this view.
  private var groupedEntries: [AgentTranscriptEntry] {
    client.agentPanes.model(paneId)?.entries ?? []
  }
  private var signIn: AgentSignInState {
    AgentSignInState(
      agent: pane?.agent, authMethods: view.authMethods, failure: view.signInFailure)
  }
  private var inset: CGFloat {
    style.canonicalLayout ? PaneInteriorMetrics.cardPadding : PaneInteriorMetrics.phoneInset
  }

  public var body: some View {
    VStack(spacing: 0) {
      transcript
      if let silentSince = pane?.agent?.silentSince {
        noOutput(since: silentSince, state: pane?.state)
      }
      if signIn != .hidden {
        AgentSignInCard(
          client: client, paneId: paneId, state: signIn, style: style, inset: inset)
      }
      if let permission = view.permission {
        AgentPermissionCard(client: client, request: permission, style: style)
          .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      if let elicitation = view.elicitation {
        AgentElicitationCard(client: client, request: elicitation, style: style)
          .id(elicitation.requestId)
          .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      if AgentQueuePresentation.isShown(items: view.queue, paused: view.queuePaused) { queue }
      if let draft = client.agentWebDrafts[paneId] {
        AgentWebElementAttachmentCard(
          draft: draft, style: style,
          remove: { client.discardWebAttachmentDraft(paneId) }
        )
        .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      composer
      if showsConfiguration,
        let options = pane?.agent?.options.filter({ !AgentComposerControls.isComposerOption($0) }),
        !options.isEmpty
      {
        configuration(options)
      }
    }
    .background(style.face).foregroundStyle(style.ink)
    .overlayScrollersWithin()
    .modifier(AgentAttachmentDropTarget(attachments: attachments, style: style))
    .task(id: prompt.isEmpty) {
      guard !prompt.isEmpty else { return }
      while !Task.isCancelled && !prompt.isEmpty {
        client.setDraft(paneId, active: true)
        do { try await Task.sleep(for: .milliseconds(ClientPolicy.draftBeatMs)) } catch { return }
      }
    }
    .onChange(of: isVisible, initial: true) { _, shown in
      client.agentPaneShown(viewer, paneId, shown)
      // The tab was closed with the keyboard in this composer: it goes back to it.
      if shown, AgentPromptDrafts.shared.takeFocus(for: paneId),
        client.selectedPaneId == paneId
      {
        placeCaretAtEnd = true
        Task { @MainActor in promptFocused = true }
      }
    }
    .onChange(of: promptFocused) { _, focused in
      // The field was drawn anew and took the keyboard: it selects what is typed, and the next
      // key would replace it. The cursor goes after the draft.
      guard focused, placeCaretAtEnd else { return }
      placeCaretAtEnd = false
      AgentPromptDrafts.shared.caret(for: paneId).place(at: prompt.count, in: prompt)
    }
    .onChange(of: paneId) { old, new in
      client.agentPaneShown(viewer, old, false)
      client.agentPaneShown(viewer, new, isVisible)
    }
    .onDisappear {
      client.agentPaneShown(viewer, paneId, false)
      AgentPromptDrafts.shared.keepFocus(promptFocused, for: paneId)
      AgentPromptDrafts.shared.caret(for: paneId).reset()
      if !prompt.isEmpty { client.setDraft(paneId, active: false) }
    }
  }

  private var transcript: some View {
    ScrollViewReader { proxy in
      ScrollView {
        LazyVStack(spacing: PaneInteriorMetrics.stackSpacing) {
          if view.hasOlderMessages {
            Button(AgentTranscriptLabels.loadEarlier) { client.loadOlderAgentMessages(paneId) }
              .buttonStyle(PanePillButtonStyle(.secondary, style: style))
          }
          if let notice = view.notice {
            Label(notice, systemImage: "exclamationmark.triangle")
              .font(style.captionFont).foregroundStyle(style.warning)
              .frame(maxWidth: .infinity, alignment: .leading)
          }
          let entries = groupedEntries
          ForEach(entries) { entry in transcriptEntry(entry).id(entry.id) }
          AgentSubagentWaitingStatus(entries: entries, style: style)
          Color.clear.frame(height: 1).id(AgentConversationFollow.bottomAnchor)
        }.scrollTargetLayout()
          .padding(.horizontal, inset)
          .padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
      }
      .defaultScrollAnchor(view.scroll.place == .bottom ? .bottom : nil)
      .scrollPosition(id: $bottomVisibleId, anchor: .bottom)
      .onChange(of: bottomVisibleId) { _, id in
        if !view.scroll.isRestoring { view.scroll.record(bottomVisibleId: id) }
      }
      .task { await restoreScroll(proxy) }
      .onDisappear { view.scroll.isRestoring = true }
      .onChange(of: view.messages.count) { _, _ in
        guard follow.messagesChanged(isVisible: isVisible) else { return }
        withAnimation(.easeOut(duration: 0.18)) {
          proxy.scrollTo(AgentConversationFollow.bottomAnchor, anchor: .bottom)
        }
      }
      .task(id: revealKey) {
        let key = revealKey
        guard follow.scrollsOnReveal(key) else { return }
        await Task.yield()
        guard !Task.isCancelled else { return }
        proxy.scrollTo(AgentConversationFollow.bottomAnchor, anchor: .bottom)
        follow.revealed()
      }
    }
  }

  /// The view is new each time its workspace is shown: go back to where the person left it.
  /// Rows not laid out yet have estimated heights, so one jump only gets near a row; the
  /// next ones, once rows have been laid out, land closer.
  private func restoreScroll(_ proxy: ScrollViewProxy) async {
    view.scroll.isRestoring = true
    for _ in 0..<AgentTranscriptScroll.restorePasses {
      // Sleep throws only when the view goes away, which ends the restore.
      do { try await Task.sleep(for: .milliseconds(AgentTranscriptScroll.restorePassMs)) } catch {
        return
      }
      let rows = groupedEntries.map(\.id)
      switch view.scroll.restoration(rowIds: rows) {
      case .bottom: proxy.scrollTo(AgentConversationFollow.bottomAnchor, anchor: .bottom)
      case .row(let id): proxy.scrollTo(id, anchor: .bottom)
      }
    }
    view.scroll.isRestoring = false
  }

  /// Type-erased so subagent rows can hold nested subagent rows.
  private func transcriptEntry(_ entry: AgentTranscriptEntry) -> AnyView {
    switch entry {
    case .message(let message):
      AnyView(
        AgentTranscriptRow(
          message: message, revision: view.revision(of: message.localId),
          workspaceId: pane?.workspaceId ?? "", style: style
        ).equatable())
    case .subagent(let group):
      AnyView(
        AgentSubagentRow(
          group: group, style: style, disclosure: $subagents, entry: transcriptEntry))
    }
  }

  private var revealKey: AgentConversationFollow.RevealKey {
    AgentConversationFollow.RevealKey(
      isVisible: isVisible, permissionId: view.permission?.requestId,
      elicitationId: view.elicitation?.requestId)
  }

  private var queue: some View {
    let presentation = AgentQueuePresentation(items: view.queue, paused: view.queuePaused)
    return VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      HStack {
        Label(
          presentation.summary,
          systemImage: view.queuePaused
            ? "pause.circle" : "text.line.last.and.arrowtriangle.forward")
        Spacer()
        if view.queuePaused {
          Button(AgentQueuePresentation.resumeTitle) { client.resumeAgentQueue(paneId) }
            .buttonStyle(PanePillButtonStyle(.primary, style: style))
        }
      }.font(style.captionFont.weight(.semibold))
      ForEach(presentation.rows, id: \.promptId) { row in
        HStack(spacing: PaneInteriorMetrics.compactSpacing) {
          VStack(alignment: .leading, spacing: 2) {
            if let sender = row.sender {
              Text(sender).font(style.captionFont.weight(.semibold))
            }
            Text(row.text).font(style.captionFont).lineLimit(2)
            if let count = row.attachmentCount {
              Label("\(count)", systemImage: "paperclip").font(style.captionFont)
            }
          }
          Spacer()
          unavailableSteer
          Button(role: .destructive) {
            client.removeQueuedPrompt(paneId, promptId: row.promptId)
          } label: {
            Image(systemName: "xmark").foregroundStyle(style.secondary)
          }
          .buttonStyle(.plain).accessibilityLabel(AgentQueuePresentation.removeTitle)
        }
      }
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .paneInteriorFill(style.canvas)
    .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
  }

  private func noOutput(since: String, state: PaneState?) -> some View {
    TimelineView(.periodic(from: .now, by: 1)) { timeline in
      let presentation = AgentSilencePresentation(
        silentSince: since, paneState: state, now: timeline.date)
      VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
        Label(
          presentation.title,
          systemImage: presentation.escalated ? "clock.badge.exclamationmark" : "clock"
        )
        .font(style.headingFont)
        .foregroundStyle(presentation.escalated ? style.warning : style.secondary)
        Text(presentation.detail)
          .font(style.captionFont).foregroundStyle(style.secondary)
        HStack {
          Spacer()
          Button(AgentSilencePresentation.cancelTurnTitle, role: .destructive) {
            client.cancelAgent(paneId)
          }
          .buttonStyle(PanePillButtonStyle(.destructive, style: style))
        }
      }
      .padding(PaneInteriorMetrics.cardPadding)
      .paneInteriorFill(style.canvas)
      .padding(.horizontal, inset)
      .padding(.bottom, PaneInteriorMetrics.rowSpacing)
      .accessibilityIdentifier("agent-no-output")
    }
  }

  @ViewBuilder private var unavailableSteer: some View {
    let button = Button(AgentQueuePresentation.sendNowTitle) {}.buttonStyle(.borderless)
      .disabled(!AgentQueuePresentation.sendNowAvailable)
      .accessibilityHint(AgentQueuePresentation.sendNowHint)
    #if os(macOS)
      button.help(AgentQueuePresentation.sendNowHint)
    #else
      button
    #endif
  }

  private var composer: some View {
    AgentComposer(
      client: client, paneId: paneId, style: style, prompt: promptBinding, attachments: attachments,
      focus: $promptFocused, harnessControl: harnessControl, connected: client.connected,
      submit: submit
    )
    .padding(.horizontal, inset).padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
  }

  private func configuration(_ options: [ConfigOption]) -> some View {
    HStack(spacing: PaneInteriorMetrics.compactSpacing) {
      ForEach(options, id: \.id) { option in
        if option.type == .select {
          Menu {
            ForEach(option.options, id: \.value) { choice in
              Button(choice.name) {
                client.setAgentConfig(paneId, configId: option.id, value: .v0(choice.value))
              }
            }
          } label: {
            Label(configValue(option.currentValue), systemImage: configIcon(option))
          }
          .disabled(pane?.agent?.operation != nil)
        } else if case .v1(let enabled) = option.currentValue {
          Button {
            client.setAgentConfig(paneId, configId: option.id, value: .v1(!enabled))
          } label: {
            Label(option.name, systemImage: enabled ? "bolt.fill" : "bolt")
          }
          .disabled(pane?.agent?.operation != nil)
        }
      }
      Spacer()
      if pane?.agent?.operation != nil { ProgressView().controlSize(.small) }
    }.font(style.captionFont).tint(style.ink).padding(.horizontal, inset)
      .padding(.bottom, PaneInteriorMetrics.cardVerticalPadding)
  }

  private func submit() {
    let catalog = client.slashCatalog(paneId: paneId)
    let sent = AgentPromptDrafts.shared.submit(
      paneId, to: client, expanding: catalog.prompt(for:), beforeSend: onExplicitRemoteAction)
    if sent { promptFocused = true }
  }

  private func configValue(_ value: ConfigValue) -> String {
    switch value {
    case .v0(let text): text
    case .v1(let enabled): enabled ? "On" : "Off"
    }
  }

  private func configIcon(_ option: ConfigOption) -> String {
    if option.isModel { return "cpu" }
    return option.isEffort ? "brain" : "slider.horizontal.3"
  }
}
