import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

@MainActor
struct PhoneWritingTextEditor: UIViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  let accessibilityName: String
  let onEditingActivity: () -> Void
  let onSelectionActivity: (Bool) -> Void
  let onScrollActivity: () -> Void
  let onFormat: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void
  let onDestination: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeUIView(context: Context) -> UITextView {
    context.coordinator.makeTextView()
  }

  func updateUIView(_ textView: UITextView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(textView)
  }

  @MainActor final class Coordinator: NSObject, UITextViewDelegate {
    var parent: PhoneWritingTextEditor
    private var text = WritingViewportText()
    private var renderedIdentity: String?
    private var renderedFont: WritingFont?
    private var renderedTheme: TerminalColorTheme?

    init(_ parent: PhoneWritingTextEditor) {
      self.parent = parent
    }

    func makeTextView() -> UITextView {
      let textView = UITextView()
      textView.delegate = self
      textView.isEditable = true
      textView.isSelectable = true
      textView.isScrollEnabled = true
      textView.alwaysBounceVertical = true
      textView.backgroundColor = UIColor(PhonePalette.face)
      textView.textContainerInset = .zero
      textView.textContainer.lineFragmentPadding = 0
      textView.keyboardDismissMode = .interactive
      textView.autocorrectionType = .yes
      textView.spellCheckingType = .yes
      textView.indicatorStyle = .default
      return textView
    }

    func update(_ textView: UITextView) {
      guard let viewport = parent.view.viewport else { return }
      text.firstLine = viewport.fromLine
      let identity = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      let theme = SoyehtThemeStore.shared.activeTheme
      let needsContent = identity != renderedIdentity
      let needsStyle = renderedFont != parent.document.font || renderedTheme != theme
      guard needsContent || needsStyle else { return }
      if needsContent { text.source = EditorEditing.viewportText(viewport.lines) }
      let selection = textView.selectedRange
      textView.attributedText = NSAttributedString(string: text.source, attributes: attributes())
      textView.selectedRange = text.clamped(selection)
      textView.typingAttributes = attributes()
      textView.accessibilityLabel = "\(parent.accessibilityName), text document"
      renderedIdentity = identity
      renderedFont = parent.document.font
      renderedTheme = theme
    }

    func textView(
      _ textView: UITextView, shouldChangeTextIn range: NSRange,
      replacementText: String
    ) -> Bool {
      let replacement = EditorLocalReplacement(range: range, text: replacementText)
      let cursor = NSRange(location: range.location + replacementText.utf16.count, length: 0)
      guard
        let plan = EditorEditing.replacementPlan(
          text: text.source, viewportStartLine: text.firstLine,
          replacements: [replacement], selections: [cursor])
      else { return false }
      text.source = EditorEditing.applying(plan.replacements, to: text.source)
      parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: plan.edits)
      parent.onEditingActivity()
      return true
    }

    func textViewDidChangeSelection(_ textView: UITextView) {
      parent.onSelectionActivity(textView.selectedRange.length > 0)
    }

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
      parent.onScrollActivity()
    }

    func textView(
      _ textView: UITextView, editMenuForTextIn range: NSRange,
      suggestedActions: [UIMenuElement]
    ) -> UIMenu? {
      guard range.length > 0, let wireRange = wireRange(range) else {
        return UIMenu(children: suggestedActions)
      }
      let actions = PhoneWritingMetrics.chromePolicy.plan(
        kind: .text, hasSelection: true
      ).selectionActions
      let format = UIMenu(
        title: "Format", options: .displayInline,
        children: actions.compactMap { menuElement(for: $0, range: wireRange) })
      return UIMenu(children: suggestedActions + [format])
    }

    private func menuElement(
      for chromeAction: WritingChromeAction, range: SoyehtProtocol.TextRange
    ) -> UIMenuElement? {
      let title = chromeAction.title()
      if let kind = chromeAction.directFormat {
        return action(title, image: chromeAction.systemImage, kind: kind, range: range)
      }
      if let kind = chromeAction.destinationFormat {
        return destinationAction(title, image: chromeAction.systemImage, kind: kind, range: range)
      }
      guard !chromeAction.formatChoices.isEmpty else { return nil }
      return UIMenu(
        title: title, image: UIImage(systemName: chromeAction.systemImage),
        children: chromeAction.formatChoices.map {
          action($0.title, kind: $0.kind, range: range)
        })
    }

    private func action(
      _ title: String, image: String? = nil, kind: WritingFormatKind,
      range: SoyehtProtocol.TextRange
    ) -> UIAction {
      UIAction(title: title, image: image.flatMap(UIImage.init(systemName:))) { [weak self] _ in
        guard let self else { return }
        Task { @MainActor in self.parent.onFormat(kind, range) }
      }
    }

    private func destinationAction(
      _ title: String, image: String, kind: WritingFormatKind,
      range: SoyehtProtocol.TextRange
    ) -> UIAction {
      UIAction(title: title, image: UIImage(systemName: image)) { [weak self] _ in
        guard let self else { return }
        Task { @MainActor in self.parent.onDestination(kind, range) }
      }
    }

    private func wireRange(_ range: NSRange) -> SoyehtProtocol.TextRange? {
      guard let start = text.position(utf16Offset: range.location),
        let end = text.position(utf16Offset: NSMaxRange(range))
      else { return nil }
      return SoyehtProtocol.TextRange(start: start, end: end)
    }

    private func attributes() -> [NSAttributedString.Key: Any] {
      let paragraph = NSMutableParagraphStyle()
      paragraph.minimumLineHeight = PaneLayoutContentMetrics.textLineHeight
      paragraph.maximumLineHeight = PaneLayoutContentMetrics.textLineHeight
      return [
        .font: PhoneWritingMetrics.uiFont(for: parent.document.font),
        .foregroundColor: UIColor(PhonePalette.ink),
        .paragraphStyle: paragraph,
      ]
    }
  }
}
