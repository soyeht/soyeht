import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorNativeViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var selectedPosition: TextPosition?
  let onDefinition: (TextPosition) -> Void
  let onReferences: (TextPosition) -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeNSView(context: Context) -> NSScrollView {
    context.coordinator.makeScrollView()
  }

  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(view)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate, EditorTextInputDelegate {
    var parent: EditorNativeViewport
    private let textView = EditorTextView()
    private var ruler: EditorLineRuler?
    private var renderedIdentity: String?
    private var viewportStart: UInt32 = 0
    private var language = EditorLanguage.plain_text

    init(_ parent: EditorNativeViewport) {
      self.parent = parent
    }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.hasHorizontalScroller = true
      scroll.autohidesScrollers = true
      scroll.borderType = .noBorder
      scroll.documentView = textView
      scroll.hasVerticalRuler = true
      scroll.rulersVisible = true
      configureTextView()
      let ruler = EditorLineRuler(scrollView: scroll, textView: textView)
      scroll.verticalRulerView = ruler
      self.ruler = ruler
      return scroll
    }

    func update(_ view: EditorFileView) {
      guard let viewport = view.viewport else { return }
      viewportStart = viewport.fromLine
      language = view.file.language
      textView.inputDelegate = self
      ruler?.update(
        startLine: viewport.fromLine, lineCount: viewport.lines.count,
        changedLines: Set(view.changedLines), selectedLine: parent.selectedPosition?.line)
      let identity =
        "\(viewport.file.fileId):\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine):\(diagnosticIdentity(view.diagnostics))"
      guard renderedIdentity != identity else { return }
      renderedIdentity = identity
      let selection = textView.selectedRanges
      textView.textStorage?.setAttributedString(attributed(viewport, diagnostics: view.diagnostics))
      textView.selectedRanges = validSelections(selection, length: textView.string.utf16.count)
      textView.needsDisplay = true
      ruler?.needsDisplay = true
    }

    func insert(_ inserted: String, ranges: [NSRange]) -> Bool {
      guard
        let plan = EditorEditing.insertionPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          inserted: inserted, language: language)
      else { return false }
      apply(plan)
      return true
    }

    func delete(_ ranges: [NSRange], backward: Bool) -> Bool {
      guard
        let plan = EditorEditing.deletionPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          backward: backward, language: language)
      else { return false }
      apply(plan)
      return true
    }

    func toggleComment(_ ranges: [NSRange]) {
      guard
        let plan = EditorEditing.toggleCommentPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          language: language)
      else { return }
      apply(plan)
    }

    func selectNextOccurrence(_ ranges: [NSRange]) {
      var selections = ranges
      if selections.first?.length == 0, let offset = selections.first?.location,
        let word = EditorEditing.wordRange(text: textView.string, at: offset)
      {
        selections = [word]
      } else if let next = EditorEditing.nextOccurrence(text: textView.string, ranges: selections) {
        selections.append(next)
      }
      textView.selectedRanges = selections.map(NSValue.init(range:))
      selectionChanged()
    }

    func addCursor(at offset: Int) {
      var selections = textView.selectedRanges.map(\.rangeValue)
      let added = NSRange(location: offset, length: 0)
      guard !selections.contains(added) else { return }
      selections.append(added)
      textView.selectedRanges = selections.sorted { $0.location < $1.location }.map(
        NSValue.init(range:))
      selectionChanged()
    }

    func goToDefinition(at offset: Int) {
      guard let position = position(at: offset) else { return }
      parent.onDefinition(position)
    }

    func findReferences(at offset: Int) {
      guard let position = position(at: offset) else { return }
      parent.onReferences(position)
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      selectionChanged()
    }

    private func configureTextView() {
      textView.delegate = self
      textView.inputDelegate = self
      textView.isRichText = false
      textView.importsGraphics = false
      textView.allowsUndo = true
      textView.usesFindPanel = true
      textView.isAutomaticQuoteSubstitutionEnabled = false
      textView.isAutomaticDashSubstitutionEnabled = false
      textView.isAutomaticTextReplacementEnabled = false
      textView.isAutomaticSpellingCorrectionEnabled = false
      textView.isContinuousSpellCheckingEnabled = false
      textView.drawsBackground = true
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
      textView.textContainerInset = NSSize(width: 10, height: 10)
      textView.minSize = NSSize(width: 0, height: 0)
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = true
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = false
      textView.textContainer?.containerSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.typingAttributes = baseAttributes()
    }

    private func apply(_ plan: EditorInputPlan) {
      let changes = plan.replacements.filter { $0.range.length > 0 || !$0.text.isEmpty }
      if !changes.isEmpty {
        let storage = textView.textStorage
        storage?.beginEditing()
        for replacement in changes.sorted(by: { $0.range.location > $1.range.location }) {
          storage?.replaceCharacters(in: replacement.range, with: replacement.text)
        }
        storage?.endEditing()
        textView.didChangeText()
      }
      textView.selectedRanges = plan.selections.map(NSValue.init(range:))
      selectionChanged()
      ruler?.setLineCount(textView.string.components(separatedBy: "\n").count)
      if !plan.edits.isEmpty {
        parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: plan.edits)
      }
    }

    private func attributed(
      _ viewport: FileReadResult, diagnostics: [FileDiagnostic]
    ) -> NSAttributedString {
      let text = EditorEditing.viewportText(viewport.lines)
      let result = NSMutableAttributedString(string: text, attributes: baseAttributes())
      var offset = 0
      for line in viewport.lines {
        let length = line.text.utf16.count
        for span in line.highlights {
          let start = min(Int(span.startColumn), length)
          let end = min(max(Int(span.endColumn), start), length)
          result.addAttribute(
            .foregroundColor, value: highlightColor(span.kind),
            range: NSRange(location: offset + start, length: end - start))
        }
        offset += length + 1
      }
      for diagnostic in diagnostics {
        guard let range = EditorDiagnostics.localRange(diagnostic, in: viewport) else { continue }
        result.addAttributes(
          [
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .underlineColor: diagnosticColor(diagnostic.severity),
            .toolTip: "\(diagnostic.source) · \(diagnostic.message)",
          ], range: range)
      }
      return result
    }

    private func diagnosticIdentity(_ diagnostics: [FileDiagnostic]) -> String {
      diagnostics.map {
        "\($0.diagnosticId):\($0.range.start.line):\($0.range.start.column):\($0.range.end.line):\($0.range.end.column):\($0.severity.rawValue):\($0.message)"
      }.joined(separator: "|")
    }

    private func diagnosticColor(_ severity: DiagnosticSeverity) -> NSColor {
      switch severity {
      case .error: NSColor(MacTheme.danger)
      case .warning: NSColor(MacTheme.warning)
      case .information: NSColor(MacTheme.accent)
      case .hint: NSColor(MacTheme.muted)
      }
    }

    private func baseAttributes() -> [NSAttributedString.Key: Any] {
      let paragraph = NSMutableParagraphStyle()
      paragraph.minimumLineHeight = 19
      paragraph.maximumLineHeight = 19
      return [
        .font: NSFont(name: "JetBrains Mono", size: 11.5)
          ?? NSFont.monospacedSystemFont(ofSize: 11.5, weight: .regular),
        .foregroundColor: NSColor(MacTheme.ink),
        .paragraphStyle: paragraph,
      ]
    }

    private func highlightColor(_ kind: HighlightKind) -> NSColor {
      switch kind {
      case .keyword, .function, .operator: NSColor(MacTheme.accent)
      case .type: NSColor(MacTheme.syntaxType)
      case .number, .constant: NSColor(MacTheme.warning)
      case .comment: NSColor(MacTheme.muted)
      case .string: NSColor(MacTheme.success)
      case .property, .punctuation, .variable: NSColor(MacTheme.ink)
      }
    }

    private func validSelections(_ values: [NSValue], length: Int) -> [NSValue] {
      let valid = values.filter { NSMaxRange($0.rangeValue) <= length }
      return valid.isEmpty ? [NSValue(range: NSRange(location: 0, length: 0))] : valid
    }

    private func selectionChanged() {
      let location = textView.selectedRange().location
      parent.selectedPosition = position(at: location)
      ruler?.selectedLine = parent.selectedPosition?.line
      ruler?.needsDisplay = true
    }

    private func position(at offset: Int) -> TextPosition? {
      EditorEditing.textPosition(
        text: textView.string, viewportStartLine: viewportStart, utf16Offset: offset)
    }
  }
}

@MainActor private protocol EditorTextInputDelegate: AnyObject {
  func insert(_ inserted: String, ranges: [NSRange]) -> Bool
  func delete(_ ranges: [NSRange], backward: Bool) -> Bool
  func toggleComment(_ ranges: [NSRange])
  func selectNextOccurrence(_ ranges: [NSRange])
  func addCursor(at offset: Int)
  func goToDefinition(at offset: Int)
  func findReferences(at offset: Int)
}

@MainActor private final class EditorTextView: NSTextView {
  weak var inputDelegate: EditorTextInputDelegate?
  private var contextOffset = 0

  override func insertText(_ insertString: Any, replacementRange: NSRange) {
    guard let inserted = insertString as? String,
      inputDelegate?.insert(inserted, ranges: ranges(replacementRange)) == true
    else {
      super.insertText(insertString, replacementRange: replacementRange)
      return
    }
  }

  override func deleteBackward(_ sender: Any?) {
    guard inputDelegate?.delete(selectedRanges.map(\.rangeValue), backward: true) == true else {
      super.deleteBackward(sender)
      return
    }
  }

  override func deleteForward(_ sender: Any?) {
    guard inputDelegate?.delete(selectedRanges.map(\.rangeValue), backward: false) == true else {
      super.deleteForward(sender)
      return
    }
  }

  override func performKeyEquivalent(with event: NSEvent) -> Bool {
    let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let key = event.charactersIgnoringModifiers?.lowercased()
    if flags.contains(.command), key == "d" {
      inputDelegate?.selectNextOccurrence(selectedRanges.map(\.rangeValue))
      return true
    }
    if flags.contains(.command), key == "/" {
      inputDelegate?.toggleComment(selectedRanges.map(\.rangeValue))
      return true
    }
    if flags.contains(.command), key == "s" { return true }
    return super.performKeyEquivalent(with: event)
  }

  override func mouseDown(with event: NSEvent) {
    if event.modifierFlags.contains(.command) {
      inputDelegate?.goToDefinition(at: characterOffset(for: event))
      return
    }
    guard event.modifierFlags.contains(.option), let layoutManager, let textContainer else {
      super.mouseDown(with: event)
      return
    }
    var point = convert(event.locationInWindow, from: nil)
    point.x -= textContainerOrigin.x
    point.y -= textContainerOrigin.y
    let glyph = layoutManager.glyphIndex(for: point, in: textContainer)
    let offset = min(layoutManager.characterIndexForGlyph(at: glyph), string.utf16.count)
    inputDelegate?.addCursor(at: offset)
  }

  override func menu(for event: NSEvent) -> NSMenu? {
    contextOffset = characterOffset(for: event)
    let menu = (super.menu(for: event)?.copy() as? NSMenu) ?? NSMenu()
    if !menu.items.isEmpty { menu.addItem(.separator()) }
    let definition = NSMenuItem(
      title: "Go to Definition", action: #selector(openDefinitionFromMenu), keyEquivalent: "")
    definition.target = self
    menu.addItem(definition)
    let references = NSMenuItem(
      title: "Find References", action: #selector(findReferencesFromMenu), keyEquivalent: "")
    references.target = self
    menu.addItem(references)
    return menu
  }

  @objc private func openDefinitionFromMenu() {
    inputDelegate?.goToDefinition(at: contextOffset)
  }

  @objc private func findReferencesFromMenu() {
    inputDelegate?.findReferences(at: contextOffset)
  }

  private func characterOffset(for event: NSEvent) -> Int {
    guard let layoutManager, let textContainer else { return selectedRange().location }
    var point = convert(event.locationInWindow, from: nil)
    point.x -= textContainerOrigin.x
    point.y -= textContainerOrigin.y
    let glyph = layoutManager.glyphIndex(for: point, in: textContainer)
    return min(layoutManager.characterIndexForGlyph(at: glyph), string.utf16.count)
  }

  private func ranges(_ replacement: NSRange) -> [NSRange] {
    if replacement.location != NSNotFound { return [replacement] }
    return selectedRanges.map(\.rangeValue)
  }
}

@MainActor private final class EditorLineRuler: NSRulerView {
  private weak var textView: NSTextView?
  private var startLine: UInt32 = 0
  private var lineCount = 0
  private var changedLines: Set<UInt32> = []
  var selectedLine: UInt32?

  init(scrollView: NSScrollView, textView: NSTextView) {
    self.textView = textView
    super.init(scrollView: scrollView, orientation: .verticalRuler)
    clientView = textView
    ruleThickness = 44
  }

  required init(coder: NSCoder) {
    super.init(coder: coder)
  }

  func update(
    startLine: UInt32, lineCount: Int, changedLines: Set<UInt32>, selectedLine: UInt32?
  ) {
    self.startLine = startLine
    self.lineCount = lineCount
    self.changedLines = changedLines
    self.selectedLine = selectedLine
    needsDisplay = true
  }

  func setLineCount(_ value: Int) {
    lineCount = value
    needsDisplay = true
  }

  override func drawHashMarksAndLabels(in rect: NSRect) {
    NSColor(MacTheme.face).setFill()
    rect.fill()
    guard let textView, let scrollView else { return }
    let offset = scrollView.contentView.bounds.minY
    let inset = textView.textContainerInset.height
    let font =
      NSFont(name: "JetBrains Mono", size: 11)
      ?? NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
    let attributes: [NSAttributedString.Key: Any] = [
      .font: font, .foregroundColor: NSColor(MacTheme.muted),
    ]
    for index in 0..<lineCount {
      let line = startLine + UInt32(clamping: index)
      let y = inset + CGFloat(index) * 19 - offset
      guard y + 19 >= rect.minY, y <= rect.maxY else { continue }
      if selectedLine == line {
        NSColor(MacTheme.well).setFill()
        NSRect(x: 0, y: y, width: ruleThickness, height: 19).fill()
      }
      if changedLines.contains(line) {
        NSColor(MacTheme.diffAdded).setFill()
        NSRect(x: 0, y: y, width: 3, height: 19).fill()
      }
      let label = "\(UInt64(line) + 1)" as NSString
      let size = label.size(withAttributes: attributes)
      label.draw(
        at: NSPoint(x: ruleThickness - size.width - 10, y: y + 2), withAttributes: attributes)
    }
  }
}
