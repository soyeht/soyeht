import SwiftUI

/// The words the person teaches the engine: project names, commands, people. Flat rows on the
/// canvas, palette roles only.
struct DictationVocabularyView: View {
  let store: DictationVocabularyStore
  let style: AgentConversationStyle
  @State private var draft = ""
  @State private var notice: String?

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      Text("Vocabulary").font(style.headingFont).foregroundStyle(style.ink)
      Text("Words the engine should prefer when it hears something that sounds alike.")
        .font(style.captionFont).foregroundStyle(style.secondary)
      HStack(spacing: PaneInteriorMetrics.compactSpacing) {
        // AppKit draws a plain field's own placeholder in the system color, so it is drawn here
        // in the palette's secondary ink instead.
        TextField("", text: $draft)
          .textFieldStyle(.plain).font(style.bodyFont).foregroundStyle(style.ink)
          .overlay(alignment: .leading) {
            if draft.isEmpty {
              Text("Add a word or phrase").font(style.bodyFont).foregroundStyle(style.secondary)
                .allowsHitTesting(false).accessibilityHidden(true)
            }
          }
          .accessibilityLabel("Add a word or phrase")
          .onSubmit(add)
          .accessibilityIdentifier("dictation-vocabulary-field")
        Button("Add", action: add)
          .buttonStyle(PanePillButtonStyle(.primary, style: style))
          .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
          .accessibilityIdentifier("dictation-vocabulary-add")
      }
      .padding(PaneInteriorMetrics.cardPadding)
      .background(style.canvas, in: RoundedRectangle(cornerRadius: PaneInteriorMetrics.wellRadius))
      if let notice {
        Text(notice).font(style.captionFont).foregroundStyle(style.muted)
      }
      ForEach(store.phrases, id: \.self) { phrase in
        HStack {
          Text(phrase).font(style.bodyFont).foregroundStyle(style.ink)
          Spacer()
          Button {
            store.remove(phrase)
          } label: {
            Image(systemName: "xmark").foregroundStyle(style.secondary)
          }
          .buttonStyle(.plain)
          .accessibilityLabel("Remove \(phrase)")
        }
        .padding(.vertical, PaneInteriorMetrics.tightSpacing)
      }
    }
  }

  private func add() {
    switch store.add(draft) {
    case .added:
      draft = ""
      notice = nil
    case .duplicate: notice = "That word is already in the list."
    case .full: notice = "The list holds \(DictationVocabulary.phraseLimit) entries."
    case .empty: break
    }
  }
}
