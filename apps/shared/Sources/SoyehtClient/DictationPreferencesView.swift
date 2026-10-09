import SwiftUI

/// The key that switches the dictation engine from the keyboard: the Mac menu and the iPad's
/// hardware keyboard use this one definition. Command-Shift-E is the editor's project panel.
public enum DictationShortcut {
  public static let key: KeyEquivalent = "e"
  public static let modifiers: EventModifiers = [.control, .command]
  public static let label = "⌃⌘E"
}

/// Dictation in Preferences, on the Mac and the phone: the engines of the catalogue, the
/// language of the selected engine and the vocabulary. Every row is generated from the
/// registry, so an engine added there appears here without a change to this view. Flat, and
/// only palette roles. It does not scroll: the page around it supplies the title, `summary` and
/// the scrolling.
public struct DictationPreferencesView: View {
  public static let summary =
    "The engine behind the composer's mic. \(DictationShortcut.label) switches engine from the keyboard."

  let store: DictationEngineStore
  let style: AgentConversationStyle

  public init(store: DictationEngineStore, style: AgentConversationStyle) {
    self.store = store
    self.style = style
  }

  private func actions(for choice: DictationEngineStore.Choice) -> DictationEngineRow.Actions {
    DictationEngineRow.Actions(
      select: { store.select(choice.id) }, install: { store.install(engineID: choice.id) },
      remove: { store.remove(engineID: choice.id) })
  }

  public var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.stackSpacing) {
      ForEach(store.choices) { choice in
        DictationEngineRow(choice: choice, style: style, actions: actions(for: choice))
      }
      DictationLanguagePicker(store: store, style: style)
      if store.selected.usesVocabulary {
        DictationVocabularyView(store: store.vocabulary, style: style)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("dictation-preferences")
    .onDisappear { store.dismissInstallPrompt() }
  }
}

/// One engine: pick it once it is installed, and see what it needs (an install, a newer device).
struct DictationEngineRow: View {
  struct Actions {
    let select: () -> Void
    let install: () -> Void
    let remove: () -> Void
  }

  let choice: DictationEngineStore.Choice
  let style: AgentConversationStyle
  let actions: Actions

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      Button(action: actions.select) {
        HStack(alignment: .top, spacing: PaneInteriorMetrics.rowSpacing) {
          Image(systemName: choice.isSelected ? "largecircle.fill.circle" : "circle")
            .foregroundStyle(choice.isSelected ? style.accent : style.secondary)
          VStack(alignment: .leading, spacing: PaneInteriorMetrics.tightSpacing) {
            Text(choice.engine.name).font(style.bodyFont).foregroundStyle(style.ink)
            Text(choice.engine.summary).font(style.captionFont).foregroundStyle(style.secondary)
          }
          .opacity(choice.isSelectable ? 1 : PaneInteriorMetrics.disabledOpacity)
          Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .disabled(!choice.isSelectable)
      .accessibilityAddTraits(choice.isSelected ? .isSelected : [])
      .accessibilityIdentifier("dictation-engine-\(choice.id)")
      detail
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .frame(maxWidth: .infinity, alignment: .leading)
    .background(style.canvas, in: RoundedRectangle(cornerRadius: PaneInteriorMetrics.cardRadius))
    .overlay {
      if choice.isInstallPrompt {
        RoundedRectangle(cornerRadius: PaneInteriorMetrics.cardRadius)
          .stroke(style.accent, lineWidth: PaneInteriorMetrics.separatorWidth)
      }
    }
  }

  @ViewBuilder private var detail: some View {
    if let reason = choice.unavailableReason {
      Text(reason).font(style.captionFont).foregroundStyle(style.secondary)
    } else if let info = choice.engine.model, let model = choice.model {
      DictationModelStatus(info: info, store: model, style: style, actions: actions)
    }
  }
}

/// The install step of an engine with a model: what it is, its size and licence, and an Install
/// button that is the only thing that starts the download. Once installed: its state and Remove.
struct DictationModelStatus: View {
  let info: DictationModelInfo
  let store: DictationModelStore
  let style: AgentConversationStyle
  let actions: DictationEngineRow.Actions
  @Environment(\.openURL) private var openURL

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.tightSpacing) {
      switch store.phase {
      case .downloading(let fraction):
        ProgressView(value: fraction).tint(style.accent)
        note("Downloading \(Int((fraction * 100).rounded()))% of \(info.sizeDescription)")
      case .preparing:
        ProgressView().controlSize(.small).tint(style.accent)
        note("Preparing the model")
      case .ready:
        installed("Installed. Works offline.")
      case .failed(let reason):
        Text(reason).font(style.captionFont).foregroundStyle(style.danger)
        if store.isInstalled { installed("The model is on disk but did not load.") } else { offer }
      case .notInstalled:
        if store.isInstalled { installed("Installed.") } else { offer }
      }
      if let failure = store.removalFailure {
        Text("Could not remove it: \(failure)").font(style.captionFont).foregroundStyle(
          style.danger)
      }
    }
    .accessibilityElement(children: .contain)
  }

  /// Name, size and licence, then the button. Nothing downloads before it is pressed.
  private var offer: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      note("Not installed. Download size: \(info.sizeDescription).")
      license
      Button("Install", action: actions.install)
        .buttonStyle(PanePillButtonStyle(.primary, style: style))
        .accessibilityIdentifier("dictation-model-install")
    }
  }

  @ViewBuilder private var license: some View {
    if let url = info.licenseURL {
      HStack(spacing: PaneInteriorMetrics.tightSpacing) {
        note("Licence:")
        // A link draws in the system blue, which is not a palette color.
        Button(info.licenseName) { openURL(url) }
          .buttonStyle(.plain).font(style.captionFont).underline().foregroundStyle(style.accent)
      }
    } else {
      note("Licence: \(info.licenseName)")
    }
  }

  private func installed(_ text: String) -> some View {
    HStack(spacing: PaneInteriorMetrics.rowSpacing) {
      note(text)
      Spacer(minLength: 0)
      if case .failed = store.phase {
        Button("Retry", action: actions.install)
          .buttonStyle(PanePillButtonStyle(.secondary, style: style))
          .accessibilityIdentifier("dictation-model-retry")
      }
      Button("Remove download", action: actions.remove)
        .buttonStyle(PanePillButtonStyle(.destructive, style: style))
        .accessibilityIdentifier("dictation-model-remove")
    }
  }

  private func note(_ text: String) -> some View {
    Text(text).font(style.captionFont).foregroundStyle(style.muted)
  }
}

/// The languages of the selected engine; the list follows the engine.
struct DictationLanguagePicker: View {
  let store: DictationEngineStore
  let style: AgentConversationStyle

  var body: some View {
    HStack {
      Text("Language").font(style.bodyFont).foregroundStyle(style.ink)
      Spacer()
      Menu {
        ForEach(store.selected.languages) { language in
          Button(language.title) { store.selectLanguage(language) }
        }
      } label: {
        HStack(spacing: AgentComposerMetrics.chipSpacing) {
          Text(store.language.title).font(style.bodyFont).foregroundStyle(style.ink)
          Image(systemName: "chevron.up.chevron.down")
            .font(.system(size: AgentComposerMetrics.chevronSize))
            .foregroundStyle(style.secondary)
        }
      }
      .composerMenuStyle()
      .accessibilityLabel("Language")
      .accessibilityValue(store.language.title)
      .accessibilityIdentifier("dictation-language")
    }
    .padding(.horizontal, PaneInteriorMetrics.cardPadding)
  }
}
