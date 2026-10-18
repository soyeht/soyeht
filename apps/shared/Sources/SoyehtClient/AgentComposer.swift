import SoyehtProtocol
import SwiftUI

/// The approved composer (`DCzvR`, "Composer · APROVADO (Opção A)"): one flat card with the
/// message line on top and one control row below it: "+" · harness ▾ · model ▾ · effort ▾ ·
/// permission ▾, then the context ring, the mic and send (stop while the agent runs).
///
/// The row is `AgentComposerControls`, built from the pane's ACP options, so a harness shows
/// only the menus it advertises. The chips sit in one horizontal scroll view: when they do not
/// fit (an iPhone, a narrow pane) they scroll sideways, and "+", the ring, the mic and send stay
/// put. The row never switches between two layouts while a menu is open.
struct AgentComposer<HarnessControl: View>: View {
  @Bindable var client: EngineClient
  let paneId: String
  let style: AgentConversationStyle
  @Binding var prompt: String
  let attachments: AgentComposerAttachments
  let focus: FocusState<Bool>.Binding
  let harnessControl: HarnessControl
  /// The engine connection; menus and send wait for it.
  let connected: Bool
  let submit: () -> Void
  @State private var dictation = DictationController(makeSession: {
    DictationEngineStore.shared.session()
  })
  @State private var caret = AgentComposerCaret()
  @State private var slash = SlashCompletionState()

  private var pane: Pane? { client.panes.first { $0.paneId == paneId } }

  private var controls: AgentComposerControls {
    let agent = pane?.agent
    let usage = client.agentPanes[paneId]?.messages.reversed().lazy.compactMap(\.usage).first
    let name = agent.map {
      HarnessSwitchModel.control(
        current: $0.harness, switchState: client.agentPanes[paneId]?.harnessSwitch,
        harnesses: client.harnesses, paneName: pane?.name
      ).title
    }
    return AgentComposerControls(
      harnessName: name ?? "the agent", agent: agent, state: pane?.state, usage: usage,
      hasDraft: !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
      connected: connected,
      catalogue: agent.flatMap { client.agentCatalogues[$0.harness]?.models } ?? [])
  }

  var body: some View {
    let controls = controls
    VStack(alignment: .leading, spacing: AgentComposerMetrics.rowSpacing) {
      AgentComposerAttachmentRow(attachments: attachments, style: style)
      field(placeholder: controls.placeholder)
      HStack(spacing: AgentComposerMetrics.controlSpacing) {
        AgentAttachButton(attachments: attachments, style: style)
        ScrollView(.horizontal) { chips(controls).fixedSize() }.scrollIndicators(.hidden)
          .frame(height: AgentComposerMetrics.controlHeight)
        if controls.showsContext {
          AgentContextGauge(
            fraction: controls.contextFraction, percentage: controls.contextPercentage,
            style: style
          )
          .fixedSize()
        }
        AgentDictationButton(dictation: dictation, style: style)
        sendButton(controls.action)
      }
    }
    .padding(.horizontal, PaneInteriorMetrics.cardPadding)
    .padding(.vertical, PaneInteriorMetrics.cardVerticalPadding)
    .paneInteriorFill(style.canvas, radius: PaneInteriorMetrics.composerRadius)
    .slashCompletionList(
      client: client, paneId: paneId, text: $prompt, state: slash, style: style,
      accept: acceptSlash
    )
    .onAppear { dictation.onTranscript = insert }
    .onDisappear { dictation.cancel() }
  }

  private func chips(_ controls: AgentComposerControls) -> some View {
    HStack(spacing: AgentComposerMetrics.controlSpacing) {
      harnessControl
      if controls.showsThinkingSeparator { separator }
      if let model = controls.model {
        AgentConfigMenuChip(
          menu: model, enabled: controls.configurable, style: style, accessibilityName: "Model",
          select: { select($0, in: model) })
      }
      if let effort = controls.effort {
        AgentConfigMenuChip(
          menu: effort, enabled: controls.configurable, style: style,
          accessibilityName: "Reasoning effort", select: { select($0, in: effort) })
      }
      if controls.showsPermissionSeparator { separator }
      if let modes = controls.permission {
        AgentPermissionModeMenu(
          modes: modes, enabled: controls.configurable, style: style,
          select: { client.setPermissionMode(paneId, to: $0) })
      }
    }
  }

  private var separator: some View {
    Text("·").font(style.controlFont).foregroundStyle(style.secondary)
      .fixedSize().accessibilityHidden(true)
  }

  /// AppKit draws a plain field's own placeholder in the system color, so the placeholder is
  /// drawn here in the palette's secondary ink instead.
  private func field(placeholder: String) -> some View {
    caret.textField(text: $prompt)
      .overlay(alignment: .topLeading) {
        if prompt.isEmpty {
          Text(placeholder).foregroundStyle(style.secondary)
            .allowsHitTesting(false).accessibilityHidden(true)
        }
      }
      .accessibilityLabel(placeholder)
      .lineLimit(1...6).textFieldStyle(.plain).font(style.bodyFont)
      .foregroundStyle(style.ink).focused(focus)
      .onSubmit(submit)
      .onKeyPress(.tab, phases: .down) { press in
        guard press.modifiers.contains(.shift) else { return .ignored }
        client.cyclePermissionMode(paneId)
        return .handled
      }
      .slashCompletionKeys(
        client: client, paneId: paneId, text: $prompt, state: slash, accept: acceptSlash
      )
      #if os(macOS)
        .modifier(AgentComposerPaste(isFocused: focus.wrappedValue, attachments: attachments))
      #endif
  }

  /// On a paired device Return adds a line to the multi-line prompt, so Command-Return
  /// sends from a hardware keyboard. The Mac keeps Return to send.
  @ViewBuilder private func sendButton(_ action: AgentComposerAction) -> some View {
    let button = AgentComposerActionButton(
      action: action, style: style, send: submit, stop: { client.cancelAgent(paneId) })
    #if os(iOS)
      button.keyboardShortcut(.return, modifiers: .command)
    #else
      button
    #endif
  }

  private func select(_ value: String, in menu: AgentConfigMenu) {
    client.setAgentConfig(paneId, configId: menu.configId, value: .v0(value))
  }

  /// Puts the chosen "/" entry in the field with the caret after it, ready for arguments.
  private func acceptSlash(_ suggestion: SlashSuggestion) {
    let result = SlashCatalog.accept(suggestion)
    prompt = result.text
    caret.place(at: result.cursor, in: result.text)
    focus.wrappedValue = true
  }

  private func insert(_ transcript: String) {
    let result = DictationInsertion.insert(transcript, into: prompt, at: caret.offset(in: prompt))
    prompt = result.text
    caret.place(at: result.cursor, in: result.text)
    focus.wrappedValue = true
  }
}

/// The composer's cursor, where dictation inserts its transcript. The selection-aware text
/// field exists from macOS 15 and iOS 18; earlier systems append at the end.
@MainActor final class AgentComposerCaret {
  private var selection: Any?

  @ViewBuilder func textField(text: Binding<String>) -> some View {
    if #available(macOS 15, iOS 18, *) {
      TextField("", text: text, selection: binding, axis: .vertical)
    } else {
      TextField("", text: text, axis: .vertical)
    }
  }

  func offset(in text: String) -> Int? {
    guard #available(macOS 15, iOS 18, *), let selection = selection as? TextSelection,
      case .selection(let range) = selection.indices,
      range.upperBound == text.endIndex || text.indices.contains(range.upperBound)
    else { return nil }
    return text.distance(from: text.startIndex, to: range.upperBound)
  }

  func place(at offset: Int, in text: String) {
    guard #available(macOS 15, iOS 18, *), offset <= text.count else { return }
    selection = TextSelection(insertionPoint: text.index(text.startIndex, offsetBy: offset))
  }

  @available(macOS 15, iOS 18, *)
  private var binding: Binding<TextSelection?> {
    Binding(
      get: { self.selection as? TextSelection },
      set: { self.selection = $0 })
  }
}
