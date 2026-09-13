import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorNativeViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var selectedPosition: TextPosition?
  let navigationTarget: EditorNavigationTarget?
  let diffMarkers: [UInt32: Set<EditorDiffMarker>]
  let agentReviewLines: Set<UInt32>
  let agentCursors: [EditorAgentCursorEvent]
  let vimEnabled: Bool
  @Binding var vimMode: EditorVimMode
  @Binding var vimCommandLine: String?
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

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    private static let lineHeight: CGFloat = 19
    var parent: EditorNativeViewport
    private let textView = EditorTextView()
    private weak var scrollView: NSScrollView?
    private var ruler: EditorLineRuler?
    private var renderedIdentity: String?
    private var appliedNavigationId: String?
    private var viewportStart: UInt32 = 0
    private var language = EditorLanguage.plain_text
    private var vim = EditorVimMachine()
    private var wasVimEnabled = false
    private var requestedViewportStart: UInt32?

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
      scroll.contentView.postsBoundsChangedNotifications = true
      NotificationCenter.default.addObserver(
        self, selector: #selector(visibleBoundsChanged(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      scroll.hasVerticalRuler = true
      scroll.rulersVisible = true
      configureTextView()
      let ruler = EditorLineRuler(scrollView: scroll, textView: textView)
      scroll.verticalRulerView = ruler
      self.ruler = ruler
      scrollView = scroll
      return scroll
    }

    func update(_ view: EditorFileView) {
      guard let viewport = view.viewport else { return }
      let previousViewportStart = viewportStart
      viewportStart = viewport.fromLine
      language = view.file.language
      textView.inputDelegate = self
      textView.vimEnabled = parent.vimEnabled
      if wasVimEnabled, !parent.vimEnabled { vim.reset() }
      wasVimEnabled = parent.vimEnabled
      publishVimState()
      ruler?.update(
        startLine: viewport.fromLine, lineCount: viewport.lines.count,
        markers: parent.diffMarkers, agentReviewLines: parent.agentReviewLines,
        agentCursors: parent.agentCursors, selectedLine: parent.selectedPosition?.line)
      updateVirtualGeometry(viewport)
      let identity =
        "\(viewport.file.fileId):\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine):\(diagnosticIdentity(view.diagnostics))"
      if renderedIdentity != identity {
        renderedIdentity = identity
        let selection = textView.selectedRanges
        textView.textStorage?.setAttributedString(
          attributed(viewport, diagnostics: view.diagnostics))
        textView.selectedRanges = selections(
          afterLoading: viewport, previousStart: previousViewportStart, previous: selection)
        textView.needsDisplay = true
        ruler?.needsDisplay = true
      }
      applyNavigation(viewport)
      requestViewportIfNeeded()
    }

    @objc private func visibleBoundsChanged(_ notification: Notification) {
      requestViewportIfNeeded()
    }

    private func updateVirtualGeometry(_ viewport: FileReadResult) {
      guard let scrollView else { return }
      let top = 10 + CGFloat(viewport.fromLine) * Self.lineHeight
      textView.textContainerInset = NSSize(width: 10, height: top)
      let documentHeight = 20 + CGFloat(viewport.file.lineCount) * Self.lineHeight
      let height = max(scrollView.contentSize.height, documentHeight)
      textView.minSize = NSSize(width: 0, height: height)
      if abs(textView.frame.height - height) > 0.5 {
        textView.setFrameSize(
          NSSize(width: max(scrollView.contentSize.width, textView.frame.width), height: height))
      }
      if requestedViewportStart == viewport.fromLine { requestedViewportStart = nil }
    }

    private func requestViewportIfNeeded() {
      guard let scrollView, let viewport = parent.view.viewport,
        requestedViewportStart == nil
      else { return }
      let visibleTop = max(0, scrollView.contentView.bounds.minY - 10)
      let visibleFrom = UInt32(clamping: Int(visibleTop / Self.lineHeight))
      let visibleLines = UInt32(
        clamping: Int(ceil(scrollView.contentSize.height / Self.lineHeight)) + 2)
      guard
        let start = EditorViewportPaging.requestedStart(
          visibleFrom: visibleFrom, visibleLines: visibleLines,
          loadedFrom: viewport.fromLine, loadedTo: viewport.toLine,
          totalLines: viewport.file.lineCount)
      else { return }
      requestedViewportStart = start
      let fileId = viewport.file.fileId
      Task { @MainActor in
        do {
          try await parent.client.loadEditorViewport(fileId: fileId, fromLine: start)
        } catch {
          parent.client.problem = error.localizedDescription
        }
        requestedViewportStart = nil
      }
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

    func handleVim(_ key: EditorVimKey, ranges: [NSRange]) -> Bool {
      guard parent.vimEnabled else { return false }
      let result = vim.handle(
        key, text: textView.string, viewportStartLine: viewportStart,
        selections: ranges, language: language)
      if let plan = result.plan { apply(plan) }
      if let selections = result.selections {
        textView.selectedRanges = selections.map(NSValue.init(range:))
        selectionChanged()
      }
      if let effect = result.effect { perform(effect) }
      publishVimState()
      return result.handled
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

    private func selections(
      afterLoading viewport: FileReadResult, previousStart: UInt32, previous: [NSValue]
    ) -> [NSValue] {
      guard previousStart != viewport.fromLine else {
        return validSelections(previous, length: textView.string.utf16.count)
      }
      guard let position = parent.selectedPosition,
        let offset = localOffset(position, in: viewport)
      else { return [NSValue(range: NSRange(location: 0, length: 0))] }
      return [NSValue(range: NSRange(location: offset, length: 0))]
    }

    private func applyNavigation(_ viewport: FileReadResult) {
      guard let target = parent.navigationTarget, target.fileId == viewport.file.fileId,
        target.id != appliedNavigationId,
        let offset = localOffset(target.position, in: viewport)
      else { return }
      appliedNavigationId = target.id
      let range = NSRange(location: offset, length: 0)
      textView.setSelectedRange(range)
      textView.scrollRangeToVisible(range)
      selectionChanged()
    }

    private func localOffset(_ position: TextPosition, in viewport: FileReadResult) -> Int? {
      guard position.line >= viewport.fromLine, position.line < viewport.toLine else { return nil }
      let index = Int(position.line - viewport.fromLine)
      guard index < viewport.lines.count else { return nil }
      let prefix = viewport.lines.prefix(index).reduce(0) { $0 + $1.text.utf16.count + 1 }
      let column = min(Int(position.column), viewport.lines[index].text.utf16.count)
      return prefix + column
    }

    private func selectionChanged() {
      let location = textView.selectedRange().location
      parent.selectedPosition = position(at: location)
      ruler?.selectedLine = parent.selectedPosition?.line
      ruler?.needsDisplay = true
    }

    private func publishVimState() {
      parent.vimMode = vim.mode
      parent.vimCommandLine = vim.commandLine
    }

    private func perform(_ effect: EditorVimEffect) {
      let fileId = parent.view.file.fileId
      switch effect {
      case .close:
        parent.client.closePane(parent.view.file.paneId)
      case .save:
        Task { @MainActor in await report { try await self.parent.client.saveEditorFile(fileId) } }
      case .reload:
        Task { @MainActor in await report { try await self.parent.client.reloadEditorFile(fileId) }
        }
      case .substitute(let pattern, let replacement, let everyMatch):
        Task { @MainActor in
          await report {
            try await self.parent.client.reloadEditorFile(fileId)
            guard let revision = self.parent.client.editorFiles[fileId]?.file.revision else {
              throw ClientFailure.invalidSnapshot
            }
            _ = try await self.parent.client.substituteEditorFile(
              fileId: fileId, baseRevision: revision, pattern: pattern,
              replacement: replacement, everyMatch: everyMatch)
          }
        }
      }
    }

    private func report(_ operation: () async throws -> Void) async {
      do { try await operation() } catch { parent.client.problem = error.localizedDescription }
    }

    private func position(at offset: Int) -> TextPosition? {
      EditorEditing.textPosition(
        text: textView.string, viewportStartLine: viewportStart, utf16Offset: offset)
    }
  }
}

@MainActor private final class EditorTextView: NSTextView {
  weak var inputDelegate: EditorNativeViewport.Coordinator?
  var vimEnabled = false
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

  override func keyDown(with event: NSEvent) {
    let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    guard vimEnabled, !flags.contains(.command), !flags.contains(.control),
      !flags.contains(.option), let key = vimKey(event),
      inputDelegate?.handleVim(key, ranges: selectedRanges.map(\.rangeValue)) == true
    else {
      super.keyDown(with: event)
      return
    }
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

  private func vimKey(_ event: NSEvent) -> EditorVimKey? {
    switch event.keyCode {
    case 36, 76: return .enter
    case 51: return .backspace
    case 53: return .escape
    default:
      guard let characters = event.characters, !characters.isEmpty else { return nil }
      return .character(characters)
    }
  }
}

@MainActor private final class EditorLineRuler: NSRulerView {
  private weak var textView: NSTextView?
  private var startLine: UInt32 = 0
  private var lineCount = 0
  private var diffMarkers: [UInt32: Set<EditorDiffMarker>] = [:]
  private var agentReviewLines: Set<UInt32> = []
  private var agentCursors: [UInt32: String] = [:]
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
    startLine: UInt32, lineCount: Int, markers: [UInt32: Set<EditorDiffMarker>],
    agentReviewLines: Set<UInt32>, agentCursors: [EditorAgentCursorEvent], selectedLine: UInt32?
  ) {
    self.startLine = startLine
    self.lineCount = lineCount
    diffMarkers = markers
    self.agentReviewLines = agentReviewLines
    self.agentCursors = Dictionary(
      agentCursors.map { ($0.position.line, $0.name) }, uniquingKeysWith: { first, _ in first })
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
      drawMarkers(diffMarkers[line] ?? [], y: y)
      drawAgentFacts(line: line, y: y)
      let label = "\(UInt64(line) + 1)" as NSString
      let size = label.size(withAttributes: attributes)
      label.draw(
        at: NSPoint(x: ruleThickness - size.width - 10, y: y + 2), withAttributes: attributes)
    }
  }

  private func drawAgentFacts(line: UInt32, y: CGFloat) {
    if agentReviewLines.contains(line) {
      NSColor(MacTheme.agentBlue).setFill()
      NSRect(x: 4, y: y, width: 3, height: 19).fill()
    }
    guard let name = agentCursors[line] else { return }
    NSColor(MacTheme.accent).setFill()
    NSBezierPath(ovalIn: NSRect(x: 9, y: y + 7, width: 6, height: 6)).fill()
    toolTip = "\(name) is editing line \(UInt64(line) + 1)"
  }

  private func drawMarkers(_ markers: Set<EditorDiffMarker>, y: CGFloat) {
    let ordered = markers.sorted { $0.rawValue < $1.rawValue }
    guard !ordered.isEmpty else { return }
    let height = 19 / CGFloat(ordered.count)
    for (index, marker) in ordered.enumerated() {
      markerColor(marker).setFill()
      NSRect(x: 0, y: y + CGFloat(index) * height, width: 3, height: height).fill()
    }
  }

  private func markerColor(_ marker: EditorDiffMarker) -> NSColor {
    switch marker {
    case .added: NSColor(MacTheme.diffAdded)
    case .modified: NSColor(MacTheme.warning)
    case .deleted: NSColor(MacTheme.danger)
    }
  }
}
