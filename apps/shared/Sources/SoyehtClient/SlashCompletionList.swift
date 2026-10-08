import Observation
import SoyehtProtocol
import SwiftUI

/// The composer's "/" list. The message field owns the keys: arrows move the selection,
/// Return or Tab puts the choice in the field, Escape closes the list. The list itself floats
/// above the whole composer card, anchored to its top edge, so it never covers the message
/// line or the control row; a tap chooses on touch screens.
@MainActor @Observable final class SlashCompletionState {
  var selection = 0
  var dismissed: String?

  static let visibleRows = 6

  func matches(client: EngineClient, paneId: String, text: String) -> [SlashSuggestion] {
    guard let query = SlashCatalog.query(in: text), query != dismissed else { return [] }
    return Array(client.slashCatalog(paneId: paneId).matches(query).prefix(Self.visibleRows))
  }

  func move(_ step: Int, in matches: [SlashSuggestion]) -> KeyPress.Result {
    guard !matches.isEmpty else { return .ignored }
    selection = (min(selection, matches.count - 1) + step + matches.count) % matches.count
    return .handled
  }

  /// The suggestion Return, Tab or a tap would put in the field.
  func selected(in matches: [SlashSuggestion]) -> SlashSuggestion? {
    matches.isEmpty ? nil : matches[min(selection, matches.count - 1)]
  }
}

/// The keys of the "/" list, on the message field.
private struct SlashCompletionKeys: ViewModifier {
  @Bindable var client: EngineClient
  let paneId: String
  @Binding var text: String
  let state: SlashCompletionState

  func body(content: Content) -> some View {
    let matches = state.matches(client: client, paneId: paneId, text: text)
    content
      .onKeyPress(.downArrow) { state.move(1, in: matches) }
      .onKeyPress(.upArrow) { state.move(-1, in: matches) }
      .onKeyPress(.return) { accept(in: matches) }
      .onKeyPress(.tab, phases: .down) { press in
        press.modifiers.contains(.shift) ? .ignored : accept(in: matches)
      }
      .onKeyPress(.escape) {
        guard !matches.isEmpty else { return .ignored }
        state.dismissed = SlashCatalog.query(in: text)
        return .handled
      }
      .onChange(of: text) { _, _ in state.selection = 0 }
      .task(id: client.connectionRevision) { await client.loadSlashSources(paneId: paneId) }
  }

  private func accept(in matches: [SlashSuggestion]) -> KeyPress.Result {
    guard let suggestion = state.selected(in: matches) else { return .ignored }
    text = SlashCatalog.accept(suggestion)
    return .handled
  }
}

/// The "/" list floating above the composer card: as wide as the card, its bottom edge
/// `compactSpacing` above the card's top edge.
private struct SlashCompletionList: ViewModifier {
  @Bindable var client: EngineClient
  let paneId: String
  @Binding var text: String
  let state: SlashCompletionState
  let style: AgentConversationStyle

  func body(content: Content) -> some View {
    let matches = state.matches(client: client, paneId: paneId, text: text)
    content
      .overlay(alignment: Alignment(horizontal: .center, vertical: .slashListAnchor)) {
        if !matches.isEmpty {
          list(matches).alignmentGuide(.slashListAnchor) {
            $0[.bottom] + PaneInteriorMetrics.compactSpacing
          }
        }
      }
      .zIndex(matches.isEmpty ? 0 : 1)
  }

  private func list(_ matches: [SlashSuggestion]) -> some View {
    let selected = state.selected(in: matches)
    return VStack(alignment: .leading, spacing: 0) {
      ForEach(matches) { suggestion in
        Button {
          text = SlashCatalog.accept(suggestion)
        } label: {
          row(suggestion, selected: suggestion.id == selected?.id)
        }
        .buttonStyle(.plain)
      }
    }
    .padding(PaneInteriorMetrics.tightSpacing)
    .frame(maxWidth: .infinity, alignment: .leading)
    .paneInteriorFill(style.face)
    .overlay {
      RoundedRectangle(cornerRadius: PaneInteriorMetrics.cardRadius, style: .continuous)
        .strokeBorder(style.border, lineWidth: PaneInteriorMetrics.separatorWidth)
    }
    .accessibilityIdentifier("agent-slash-completion")
  }

  private func row(_ suggestion: SlashSuggestion, selected: Bool) -> some View {
    HStack(spacing: PaneInteriorMetrics.compactSpacing) {
      Text("/" + suggestion.name).font(style.bodyFont.weight(.semibold))
        .foregroundStyle(style.ink)
      if let hint = suggestion.hint {
        Text(hint).font(style.captionFont).foregroundStyle(style.muted)
      }
      Text(suggestion.detail).font(style.captionFont).foregroundStyle(style.secondary)
        .lineLimit(1)
      Spacer(minLength: 0)
      if case .skill = suggestion.source {
        Text("Skill").font(style.captionFont).foregroundStyle(style.accent)
      }
    }
    .padding(.horizontal, PaneInteriorMetrics.compactSpacing)
    .padding(.vertical, PaneInteriorMetrics.rowVerticalPadding)
    .background(
      selected ? style.well : .clear,
      in: RoundedRectangle(cornerRadius: PaneInteriorMetrics.wellRadius)
    )
    .contentShape(Rectangle())
  }
}

extension VerticalAlignment {
  /// The composer card's top edge, where the "/" list's bottom edge (plus a gap) sits. A
  /// custom guide: overlays ignore an explicit `.top` guide, which left the list on the card.
  private enum SlashListAnchor: AlignmentID {
    static func defaultValue(in context: ViewDimensions) -> CGFloat { context[.top] }
  }

  fileprivate static let slashListAnchor = VerticalAlignment(SlashListAnchor.self)
}

extension View {
  /// The "/" keys, on the message field.
  func slashCompletionKeys(
    client: EngineClient, paneId: String, text: Binding<String>, state: SlashCompletionState
  ) -> some View {
    modifier(SlashCompletionKeys(client: client, paneId: paneId, text: text, state: state))
  }

  /// The "/" list, on the composer card it floats above.
  func slashCompletionList(
    client: EngineClient, paneId: String, text: Binding<String>, state: SlashCompletionState,
    style: AgentConversationStyle
  ) -> some View {
    modifier(
      SlashCompletionList(client: client, paneId: paneId, text: text, state: state, style: style))
  }
}
