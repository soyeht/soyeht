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

extension AgentConversationView where ControlsAccessory == EmptyView {
  public init(
    client: EngineClient, paneId: String, isVisible: Bool = true,
    showsConfiguration: Bool = true,
    style: AgentConversationStyle = .standard,
    onExplicitRemoteAction: @escaping () -> Void = {}
  ) {
    self.init(
      client: client, paneId: paneId, isVisible: isVisible,
      showsConfiguration: showsConfiguration, style: style,
      onExplicitRemoteAction: onExplicitRemoteAction, controlsAccessory: { EmptyView() })
  }
}

/// `ControlsAccessory` sits next to the permission-mode menu in the composer; a host app uses
/// it for pane controls it owns, such as the iPhone and iPad harness menu.
@MainActor public struct AgentConversationView<ControlsAccessory: View>: View {
  @Bindable private var client: EngineClient
  private let paneId: String
  private let isVisible: Bool
  private let showsConfiguration: Bool
  private let style: AgentConversationStyle
  private let onExplicitRemoteAction: () -> Void
  private let controlsAccessory: ControlsAccessory
  @State private var prompt = ""
  @State private var attachments: AgentComposerAttachments
  @State private var follow = AgentConversationFollow()
  @State private var subagents = AgentSubagentDisclosure()
  @FocusState private var promptFocused: Bool

  public init(
    client: EngineClient, paneId: String, isVisible: Bool = true,
    showsConfiguration: Bool = true,
    style: AgentConversationStyle = .standard,
    onExplicitRemoteAction: @escaping () -> Void = {},
    @ViewBuilder controlsAccessory: () -> ControlsAccessory
  ) {
    self.client = client
    self.paneId = paneId
    _attachments = State(
      initialValue: AgentComposerAttachments(linkHostFiles: !client.pairedDevice))
    self.isVisible = isVisible
    self.showsConfiguration = showsConfiguration
    self.style = style
    self.onExplicitRemoteAction = onExplicitRemoteAction
    self.controlsAccessory = controlsAccessory()
  }

  private var pane: Pane? { client.panes.first { $0.paneId == paneId } }
  private var view: AgentPaneView { client.agentPanes[paneId] ?? AgentPaneView() }
  private var isWorking: Bool {
    guard let state = pane?.state else { return false }
    return [.working, .waiting_for_you, .cancelling, .no_signal].contains(state)
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
      if !view.authMethods.isEmpty { authentication }
      if let permission = view.permission {
        AgentPermissionCard(client: client, request: permission, style: style)
          .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      if let elicitation = view.elicitation {
        AgentElicitationCard(client: client, request: elicitation, style: style)
          .id(elicitation.requestId)
          .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      if !view.queue.isEmpty || view.queuePaused { queue }
      if let draft = client.agentWebDrafts[paneId] {
        AgentWebElementAttachmentCard(
          draft: draft, style: style,
          remove: { client.discardWebAttachmentDraft(paneId) }
        )
        .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
      }
      composer
      if showsConfiguration, let options = pane?.agent?.options.filter({ !$0.isPermissionMode }),
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
        do { try await Task.sleep(for: .seconds(8)) } catch { return }
      }
    }
    .onDisappear {
      if !prompt.isEmpty { client.setDraft(paneId, active: false) }
    }
  }

  private var transcript: some View {
    ScrollViewReader { proxy in
      ScrollView {
        LazyVStack(spacing: PaneInteriorMetrics.stackSpacing) {
          if view.hasOlderMessages {
            Button("Load earlier messages") { client.loadOlderAgentMessages(paneId) }
              .buttonStyle(PanePillButtonStyle(.secondary, style: style))
          }
          if let notice = view.notice {
            Label(notice, systemImage: "exclamationmark.triangle")
              .font(style.captionFont).foregroundStyle(style.warning)
              .frame(maxWidth: .infinity, alignment: .leading)
          }
          let entries = AgentTranscriptGrouping.entries(view.messages)
          ForEach(entries) { entry in transcriptEntry(entry).id(entry.id) }
          AgentSubagentWaitingStatus(entries: entries, style: style)
          Color.clear.frame(height: 1).id(AgentConversationFollow.bottomAnchor)
        }.padding(.horizontal, inset)
          .padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
      }
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

  /// Type-erased so subagent rows can hold nested subagent rows.
  private func transcriptEntry(_ entry: AgentTranscriptEntry) -> AnyView {
    switch entry {
    case .message(let message):
      AnyView(
        AgentTranscriptRow(message: message, workspaceId: pane?.workspaceId ?? "", style: style))
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
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      HStack {
        let agentMessageCount = view.queue.filter { $0.agentMessage != nil }.count
        let countLabel =
          agentMessageCount > 0
          ? "\(agentMessageCount) \(agentMessageCount == 1 ? "message" : "messages") waiting"
          : "\(view.queue.count) queued"
        Label(
          view.queuePaused
            ? "\(countLabel) · Queue paused after interruption" : countLabel,
          systemImage: view.queuePaused
            ? "pause.circle" : "text.line.last.and.arrowtriangle.forward")
        Spacer()
        if view.queuePaused {
          Button("Resume") { client.resumeAgentQueue(paneId) }
            .buttonStyle(PanePillButtonStyle(.primary, style: style))
        }
      }.font(style.captionFont.weight(.semibold))
      ForEach(view.queue, id: \.promptId) { item in
        HStack(spacing: PaneInteriorMetrics.compactSpacing) {
          VStack(alignment: .leading, spacing: 2) {
            if let sender = item.agentMessage {
              Text("From \(sender.displayName)")
                .font(style.captionFont.weight(.semibold))
            }
            Text(item.text).font(style.captionFont).lineLimit(2)
            if let count = item.attachments?.count, count > 0 {
              Label("\(count)", systemImage: "paperclip").font(style.captionFont)
            }
          }
          Spacer()
          unavailableSteer
          Button(role: .destructive) {
            client.removeQueuedPrompt(paneId, promptId: item.promptId)
          } label: {
            Image(systemName: "xmark").foregroundStyle(style.secondary)
          }
          .buttonStyle(.plain).accessibilityLabel("Remove queued message")
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
      let elapsed = presentation.elapsed
      VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
        Label(
          presentation.escalated
            ? elapsed.map { "No output from the agent for \($0)" }
              ?? "The agent has produced no output"
            : elapsed.map { "Waiting for the first agent output · \($0)" }
              ?? "Waiting for the first agent output",
          systemImage: presentation.escalated ? "clock.badge.exclamationmark" : "clock"
        )
        .font(style.headingFont)
        .foregroundStyle(presentation.escalated ? style.warning : style.secondary)
        Text(
          elapsed == nil
            ? "The turn is active. Elapsed time is unavailable. You can keep waiting or cancel this turn."
            : presentation.escalated
              ? "The turn is still active. You can keep waiting or cancel this turn."
              : "The turn is active. Nothing from the agent has arrived yet."
        )
        .font(style.captionFont).foregroundStyle(style.secondary)
        HStack {
          Spacer()
          Button("Cancel turn", role: .destructive) { client.cancelAgent(paneId) }
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

  private var authentication: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.rowSpacing) {
      Label("Sign in to continue", systemImage: "person.badge.key")
        .font(style.headingFont).foregroundStyle(style.warning)
      #if os(macOS)
        if view.authMethods.contains(where: \.terminal) {
          Text("Choose a method to open its sign-in terminal. Enter credentials there.")
            .font(style.captionFont).foregroundStyle(style.secondary)
        }
        ForEach(view.authMethods, id: \.id) { method in
          Button {
            client.startAgentAuthentication(paneId, methodId: method.id)
          } label: {
            HStack {
              VStack(alignment: .leading, spacing: 3) {
                Text(method.name).fontWeight(.semibold)
                if let detail = method.description {
                  Text(detail).font(style.captionFont).foregroundStyle(style.secondary)
                }
              }
              Spacer()
              Image(systemName: method.terminal ? "terminal" : "arrow.right.circle")
            }
            .padding(PaneInteriorMetrics.rowSpacing)
            .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
            .contentShape(Rectangle())
          }.buttonStyle(.plain)
        }
      #else
        Text("Finish sign-in on the Mac, then return here.")
          .font(style.captionFont).foregroundStyle(style.secondary)
      #endif
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .paneInteriorFill(style.canvas)
    .padding(.horizontal, inset).padding(.bottom, PaneInteriorMetrics.rowSpacing)
  }

  @ViewBuilder private var unavailableSteer: some View {
    let button = Button("Send now") {}.buttonStyle(.borderless).disabled(true)
      .accessibilityHint("This agent only reads new messages between turns")
    #if os(macOS)
      button.help("This agent only reads new messages between turns")
    #else
      button
    #endif
  }

  /// The approved composer: one flat card holding the message field above a row with the
  /// context ring, the "effort · mode ▾" chip, host controls and the send button.
  private var composer: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      AgentComposerAttachmentRow(attachments: attachments, style: style)
      // AppKit draws a plain field's own placeholder in the system color, so the placeholder
      // is drawn here in the palette's secondary ink instead.
      TextField("", text: $prompt, axis: .vertical)
        .overlay(alignment: .topLeading) {
          if prompt.isEmpty {
            Text("Message the agent").foregroundStyle(style.secondary)
              .allowsHitTesting(false).accessibilityHidden(true)
          }
        }
        .accessibilityLabel("Message the agent")
        .lineLimit(1...6).textFieldStyle(.plain).font(style.bodyFont)
        .foregroundStyle(style.ink).focused($promptFocused)
        .onSubmit { submit() }
        .onKeyPress(.tab, phases: .down) { press in
          guard press.modifiers.contains(.shift) else { return .ignored }
          client.cyclePermissionMode(paneId)
          return .handled
        }
        .slashCompletion(client: client, paneId: paneId, text: $prompt, style: style)
        #if os(macOS)
          .modifier(AgentComposerPaste(isFocused: promptFocused, attachments: attachments))
        #endif
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        AgentAttachButton(attachments: attachments, style: style)
        if let usage = contextUsage {
          AgentContextRing(used: usage.0, total: usage.1, style: style)
        }
        if let agent = pane?.agent, let modes = AgentPermissionModes(agent: agent) {
          AgentPermissionModeMenu(
            modes: modes, effort: AgentEffortLabel.text(agent.options),
            enabled: agent.operation == nil && client.connected, style: style,
            select: { client.setPermissionMode(paneId, to: $0) })
        }
        controlsAccessory
        Spacer(minLength: 0)
        if isWorking {
          Button(role: .destructive) {
            client.cancelAgent(paneId)
          } label: {
            Label("Cancel", systemImage: "stop.fill")
          }
          .buttonStyle(PanePillButtonStyle(.destructive, style: style))
          .disabled(pane?.state == .cancelling)
        }
        sendButton
      }
      .font(style.controlFont)
    }
    .padding(.horizontal, PaneInteriorMetrics.cardPadding)
    .padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
    .paneInteriorFill(style.canvas, radius: PaneInteriorMetrics.composerRadius)
    .padding(.horizontal, inset).padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
  }

  /// On a paired device Return adds a line to the multi-line prompt, so Command-Return
  /// sends from a hardware keyboard. The Mac keeps Return to send.
  @ViewBuilder private var sendButton: some View {
    let button = Button(action: submit) {
      PaneSendButtonLabel(ink: style.ink, face: style.face, font: style.controlFont.weight(.bold))
    }.buttonStyle(.plain).disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
      .accessibilityLabel("Send message")
    #if os(iOS)
      button.keyboardShortcut(.return, modifiers: .command)
    #else
      button
    #endif
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

  private var contextUsage: (Double, Double)? {
    guard let usage = view.messages.reversed().compactMap(\.usage).first,
      let used = Double(usage.totalTokens), let totalText = usage.contextSize,
      let total = Double(totalText), total > 0
    else { return nil }
    return (min(max(used, 0), total), total)
  }

  private func submit() {
    let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty else { return }
    prompt = ""
    onExplicitRemoteAction()
    client.sendPrompt(
      paneId, text: client.slashCatalog(paneId: paneId).prompt(for: text),
      attachments: attachments.take())
    promptFocused = true
  }

  private func configValue(_ value: ConfigValue) -> String {
    switch value {
    case .v0(let text): text
    case .v1(let enabled): enabled ? "On" : "Off"
    }
  }

  private func configIcon(_ option: ConfigOption) -> String {
    switch option.category ?? option.id {
    case "model": "cpu"
    case "thought_level": "brain"
    default: "slider.horizontal.3"
    }
  }
}

/// The reasoning effort the composer chip names before the permission mode ("High · …").
enum AgentEffortLabel {
  static func text(_ options: [ConfigOption]) -> String? {
    guard let option = options.first(where: { ($0.category ?? $0.id) == "thought_level" }),
      case .v0(let value) = option.currentValue
    else { return nil }
    return option.options.first { $0.value == value }?.name ?? value
  }
}
