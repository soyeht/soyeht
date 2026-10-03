import SoyehtProtocol
import SwiftUI

/// The composer's "/" list, kept thin until the composer redesign: the list
/// floats above the field, arrows move the selection, Return or Tab puts the
/// choice in the field, Escape closes it, and a tap chooses on touch screens.
struct SlashCompletion: ViewModifier {
  @Bindable var client: EngineClient
  let paneId: String
  @Binding var text: String
  let style: AgentConversationStyle
  @State private var selection = 0
  @State private var dismissed: String?

  private static let visibleRows = 6

  private var matches: [SlashSuggestion] {
    guard let query = SlashCatalog.query(in: text), query != dismissed else { return [] }
    return Array(client.slashCatalog(paneId: paneId).matches(query).prefix(Self.visibleRows))
  }

  func body(content: Content) -> some View {
    let matches = matches
    content
      .onKeyPress(.downArrow) { move(1, in: matches) }
      .onKeyPress(.upArrow) { move(-1, in: matches) }
      .onKeyPress(.return) { accept(in: matches) }
      .onKeyPress(.tab, phases: .down) { press in
        press.modifiers.contains(.shift) ? .ignored : accept(in: matches)
      }
      .onKeyPress(.escape) {
        guard !matches.isEmpty else { return .ignored }
        dismissed = SlashCatalog.query(in: text)
        return .handled
      }
      .onChange(of: text) { _, _ in selection = 0 }
      .overlay(alignment: .topLeading) {
        if !matches.isEmpty {
          list(matches).alignmentGuide(.top) {
            $0[.bottom] + PaneInteriorMetrics.compactSpacing
          }
        }
      }
      .task(id: client.connectionRevision) { await client.loadSlashSources(paneId: paneId) }
  }

  private func list(_ matches: [SlashSuggestion]) -> some View {
    VStack(alignment: .leading, spacing: 0) {
      ForEach(Array(matches.enumerated()), id: \.element.id) { index, suggestion in
        Button {
          text = SlashCatalog.accept(suggestion)
        } label: {
          row(suggestion, selected: index == min(selection, matches.count - 1))
        }
        .buttonStyle(.plain)
      }
    }
    .padding(PaneInteriorMetrics.tightSpacing)
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

  private func move(_ step: Int, in matches: [SlashSuggestion]) -> KeyPress.Result {
    guard !matches.isEmpty else { return .ignored }
    selection = (min(selection, matches.count - 1) + step + matches.count) % matches.count
    return .handled
  }

  private func accept(in matches: [SlashSuggestion]) -> KeyPress.Result {
    guard !matches.isEmpty else { return .ignored }
    text = SlashCatalog.accept(matches[min(selection, matches.count - 1)])
    return .handled
  }
}

extension View {
  func slashCompletion(
    client: EngineClient, paneId: String, text: Binding<String>, style: AgentConversationStyle
  ) -> some View {
    modifier(SlashCompletion(client: client, paneId: paneId, text: text, style: style))
  }
}
