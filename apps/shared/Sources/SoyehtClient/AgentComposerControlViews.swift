import SoyehtProtocol
import SwiftUI

/// Measurements of the approved composer (`DCzvR`, "Composer · APROVADO (Opção A)").
public enum AgentComposerMetrics {
  /// Between the message line and the control row.
  public static let rowSpacing: CGFloat = DesignTokens.Composer.rowSpacing
  /// Between the controls of the row.
  public static let controlSpacing: CGFloat = DesignTokens.Composer.controlSpacing
  public static let controlHeight: CGFloat = DesignTokens.Composer.controlHeight
  public static let chipHorizontalPadding: CGFloat = DesignTokens.Composer.chipHorizontalPadding
  public static let chipSpacing: CGFloat = DesignTokens.Composer.chipSpacing
  public static let iconSize: CGFloat = DesignTokens.Composer.iconSize
  public static let chevronSize: CGFloat = DesignTokens.Composer.chevronSize
  public static let shieldSize: CGFloat = DesignTokens.Composer.shieldSize
  public static let harnessLogoSize: CGFloat = DesignTokens.Composer.harnessLogoSize
  public static let sendWidth: CGFloat = DesignTokens.Composer.sendWidth
  public static let stopSize: CGFloat = DesignTokens.Composer.stopSize
  public static let stopRadius: CGFloat = DesignTokens.Composer.stopRadius
  public static let ringDiameter: CGFloat = DesignTokens.Composer.ringDiameter
  /// 16 × (1 − 0.62) / 2: the design's ring is an ellipse with an inner radius of 0.62.
  public static let ringLineWidth: CGFloat = DesignTokens.Composer.ringLineWidth
  public static let contextSpacing: CGFloat = DesignTokens.Composer.contextSpacing
  public static let contextHorizontalPadding: CGFloat = DesignTokens.Composer
    .contextHorizontalPadding
}

/// A borderless chip of the control row: optional leading mark, the label and a chevron.
struct AgentComposerChip<Leading: View>: View {
  /// The menu indicator after the label ("Claude ▾").
  static var indicator: String { "chevron.down" }

  let title: String
  let emphasized: Bool
  let style: AgentConversationStyle
  @ViewBuilder let leading: Leading

  var body: some View {
    HStack(spacing: AgentComposerMetrics.chipSpacing) {
      leading
      Text(title)
        .font(emphasized ? style.controlFont.weight(.semibold) : style.controlFont)
        .foregroundStyle(emphasized ? style.ink : style.secondary)
        .lineLimit(1).truncationMode(.tail)
      Image(systemName: Self.indicator)
        .font(.system(size: AgentComposerMetrics.chevronSize - 2, weight: .semibold))
        .foregroundStyle(style.secondary)
        .frame(width: AgentComposerMetrics.chevronSize, height: AgentComposerMetrics.chevronSize)
    }
    .padding(.horizontal, AgentComposerMetrics.chipHorizontalPadding)
    .frame(height: AgentComposerMetrics.controlHeight)
    .contentShape(Capsule())
  }
}

/// The harness chip's label: the harness logo, its name in ink and a chevron. Hosts wrap it
/// in their harness switch (a popover on the Mac, a menu on the iPhone and iPad).
public struct AgentHarnessChipLabel: View {
  let control: HarnessSwitchControl
  let harnessId: String
  let style: AgentConversationStyle

  public init(control: HarnessSwitchControl, harnessId: String, style: AgentConversationStyle) {
    self.control = control
    self.harnessId = harnessId
    self.style = style
  }

  public var body: some View {
    AgentComposerChip(title: control.title, emphasized: true, style: style) {
      if control.busy {
        ProgressView().controlSize(.mini)
      } else if let provider = UsageProvider(harnessId: harnessId) {
        ProviderGlyph(provider).foregroundStyle(style.ink)
          .frame(
            width: AgentComposerMetrics.harnessLogoSize,
            height: AgentComposerMetrics.harnessLogoSize)
      }
    }
  }
}

extension UsageProvider {
  /// The provider whose logo marks a harness: "registry.claude-acp" → Claude.
  init?(harnessId: String) {
    let id = harnessId.lowercased()
    guard
      let provider = [UsageProvider.claude, .codex, .cursor].first(where: {
        id.contains($0.rawValue)
      })
    else { return nil }
    self = provider
  }
}

/// A model or effort menu, listing the harness's own choices.
struct AgentConfigMenuChip: View {
  let menu: AgentConfigMenu
  let enabled: Bool
  let style: AgentConversationStyle
  let accessibilityName: String
  let select: (String) -> Void

  var body: some View {
    Menu {
      AgentConfigMenuItems(menu: menu, select: select)
    } label: {
      AgentComposerChip(title: menu.label, emphasized: false, style: style) {
        if menu.pending != nil { ProgressView().controlSize(.mini) }
      }
    }
    .composerMenuStyle()
    .disabled(!enabled || menu.pending != nil)
    .accessibilityLabel(accessibilityName)
    .accessibilityValue(menu.label)
  }
}

/// The items of a model or effort menu: one title per choice, a check on the current one,
/// and the harness's description as the tooltip. Never a subtitle: AppKit raised an
/// exception building a title-and-subtitle item while SwiftUI refreshed the open menu
/// (VM1 crash on the model chip).
struct AgentConfigMenuItems: View {
  let menu: AgentConfigMenu
  let select: (String) -> Void

  var body: some View {
    ForEach(menu.choices) { choice in
      Button {
        select(choice.id)
      } label: {
        if choice.id == menu.current {
          Label(choice.name, systemImage: "checkmark")
        } else {
          Text(choice.name)
        }
      }
      .help(choice.detail ?? "")
      .disabled(choice.id == menu.current)
    }
  }
}

/// The context ring: the share of the context window in use and its percentage. Until the
/// harness reports its context use (ACP `usage_update`, after a turn) the ring is an empty
/// track and the percentage a dash.
struct AgentContextGauge: View {
  let fraction: Double?
  let percentage: String?
  let style: AgentConversationStyle

  private var color: Color {
    let used = fraction ?? 0
    if used > ClientPolicy.contextBandDangerAbove { return style.danger }
    return used > ClientPolicy.contextBandWarningAbove ? style.warning : style.secondary
  }

  var body: some View {
    HStack(spacing: AgentComposerMetrics.contextSpacing) {
      ZStack {
        Circle().stroke(style.border, lineWidth: AgentComposerMetrics.ringLineWidth)
        Circle().trim(from: 0, to: fraction ?? 0)
          .stroke(color, style: StrokeStyle(lineWidth: AgentComposerMetrics.ringLineWidth))
          .rotationEffect(.degrees(-90))
      }
      .padding(AgentComposerMetrics.ringLineWidth / 2)
      .frame(width: AgentComposerMetrics.ringDiameter, height: AgentComposerMetrics.ringDiameter)
      Text(percentage ?? "–").font(style.monoSmallFont.weight(.medium)).foregroundStyle(color)
    }
    .padding(.horizontal, AgentComposerMetrics.contextHorizontalPadding)
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(
      percentage.map { "Context use \($0)" } ?? "Context use not reported yet")
  }
}

/// The send button, which becomes stop while the agent runs: an `ink` pill with a `face`
/// arrow or square. It keeps the approved full `ink` while there is nothing to send; the
/// button is disabled, not dimmed.
struct AgentComposerActionButton: View {
  let action: AgentComposerAction
  let style: AgentConversationStyle
  let send: () -> Void
  let stop: () -> Void

  var body: some View {
    Button(action: isStop ? stop : send) {
      Group {
        if isStop {
          RoundedRectangle(cornerRadius: AgentComposerMetrics.stopRadius)
            .fill(style.face)
            .frame(width: AgentComposerMetrics.stopSize, height: AgentComposerMetrics.stopSize)
        } else {
          Image(systemName: "arrow.up")
            .font(.system(size: AgentComposerMetrics.iconSize - 2, weight: .bold))
            .foregroundStyle(style.face)
        }
      }
      .frame(width: AgentComposerMetrics.sendWidth, height: AgentComposerMetrics.controlHeight)
      .background(style.ink, in: Capsule())
      .contentShape(Capsule())
    }
    .buttonStyle(AgentComposerPillStyle())
    .disabled(!enabled)
    .accessibilityLabel(isStop ? "Stop the agent" : "Send message")
    .accessibilityIdentifier(isStop ? "agent-stop" : "agent-send")
  }

  private var isStop: Bool {
    if case .stop = action { return true }
    return false
  }

  private var enabled: Bool {
    switch action {
    case .send(let enabled), .stop(let enabled): enabled
    }
  }
}

/// Dims only while pressed: a disabled send keeps the approved full `ink`.
private struct AgentComposerPillStyle: ButtonStyle {
  func makeBody(configuration: Configuration) -> some View {
    configuration.label.opacity(configuration.isPressed ? PaneInteriorMetrics.pressedOpacity : 1)
  }
}

/// A round icon button of the control row ("+" and the mic).
struct AgentComposerIconButton: View {
  let systemImage: String
  let style: AgentConversationStyle
  var active = false
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      Image(systemName: systemImage)
        .font(.system(size: AgentComposerMetrics.iconSize - 2, weight: .medium))
        .foregroundStyle(active ? style.face : style.secondary)
        .frame(
          width: AgentComposerMetrics.controlHeight, height: AgentComposerMetrics.controlHeight
        )
        .background(active ? style.accent : .clear, in: Circle())
        .contentShape(Circle())
    }
    .buttonStyle(.plain)
  }
}

/// Why the mic did not listen. The bubble has a fixed width and the text wraps onto as many
/// lines as it needs: a popover sizes itself from its content's ideal size, and a text with only
/// a maximum width takes the ideal size of a single line, so the reason was cut off.
struct AgentDictationFailureBubble: View {
  static let width: CGFloat = 280

  let message: String
  let style: AgentConversationStyle

  var body: some View {
    Text(message).font(style.captionFont).foregroundStyle(style.ink)
      .multilineTextAlignment(.leading)
      .frame(width: Self.width, alignment: .leading)
      .fixedSize(horizontal: false, vertical: true)
      .padding(PaneInteriorMetrics.cardPadding)
  }
}

/// The mic: tap to dictate, tap again to stop; the transcript lands at the cursor. The name of
/// the engine that listens shows above it while it listens, and for a moment after the
/// keyboard switches engine.
struct AgentDictationButton: View {
  let dictation: DictationController
  let style: AgentConversationStyle
  var engines = DictationEngineStore.shared
  @State private var showsFailure = false
  @State private var showsSwitch = false

  var body: some View {
    AgentComposerIconButton(
      systemImage: dictation.isBusy ? "mic.fill" : "mic", style: style,
      active: dictation.isRecording, action: dictation.toggle
    )
    .disabled(dictation.state == .authorizing)
    .accessibilityLabel(dictation.isRecording ? "Stop dictation" : "Dictate")
    .accessibilityValue(dictation.isRecording ? "Recording" : "")
    .accessibilityHint(dictation.note ?? "")
    .accessibilityIdentifier("agent-dictation")
    .overlay(alignment: .top) { badge }
    .onChange(of: dictation.failure) { _, failure in showsFailure = failure != nil }
    .popover(isPresented: $showsFailure) {
      AgentDictationFailureBubble(message: dictation.failure ?? "", style: style)
        .presentationCompactAdaptation(.popover)
    }
    .task(id: engines.switchedAt) { await flashSwitch() }
    .background { engineShortcut }
  }

  @ViewBuilder private var badge: some View {
    if let name = badgeName {
      Text(name).font(style.captionFont).foregroundStyle(style.secondary)
        .fixedSize().offset(y: -AgentComposerMetrics.controlHeight / 2 - 4)
        .allowsHitTesting(false).accessibilityHidden(true)
    }
  }

  private var badgeName: String? {
    if dictation.isBusy { return dictation.badge }
    return showsSwitch ? engines.selected.badge : nil
  }

  private func flashSwitch() async {
    guard engines.switchedAt != nil else { return }
    showsSwitch = true
    do { try await Task.sleep(for: .seconds(2)) } catch { return }
    showsSwitch = false
  }

  /// A hardware keyboard on an iPad switches the engine here; the Mac does it from its menu.
  @ViewBuilder private var engineShortcut: some View {
    #if os(iOS)
      Button("Switch dictation engine") { engines.cycle() }
        .keyboardShortcut(DictationShortcut.key, modifiers: DictationShortcut.modifiers)
        .frame(width: 0, height: 0).opacity(0).accessibilityHidden(true)
    #endif
  }
}

extension View {
  /// Chips open a plain menu: no bezel and no system indicator, the chevron is drawn.
  func composerMenuStyle() -> some View {
    menuStyle(.button).buttonStyle(.plain).menuIndicator(.hidden).fixedSize()
  }
}
