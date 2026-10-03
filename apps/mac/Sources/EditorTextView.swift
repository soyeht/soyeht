import AppKit
import SoyehtClient

/// The editor text view never edits its storage on its own: every change goes
/// to the coordinator, which turns it into an engine edit, so the visible text
/// and the engine buffer cannot drift apart.
@MainActor final class EditorTextView: EditorKeyCommandTextView {
  weak var inputDelegate: EditorNativeViewport.Coordinator?
  var vimEnabled = false
  private var contextOffset = 0
  /// Set while AppKit draws input-method marked text, which stays local until
  /// the composition commits through `insertText` or `unmarkText`.
  private var composing = false

  #if SOYEHT_PERFORMANCE_METER
    var onPerformanceKeyDownStart: (() -> Void)?
    var onPerformanceKeyDownEnd: (() -> Void)?
    var onPerformanceInsertText: (() -> Void)?
    var onPerformanceDrawStart: ((NSRect) -> Void)?
    var onPerformanceDraw: ((NSRect) -> Void)?
    override func draw(_ dirtyRect: NSRect) {
      onPerformanceDrawStart?(dirtyRect)
      super.draw(dirtyRect)
      onPerformanceDraw?(dirtyRect)
    }
  #endif

  override func insertText(_ insertString: Any, replacementRange: NSRange) {
    #if SOYEHT_PERFORMANCE_METER
      onPerformanceInsertText?()
    #endif
    // AppKit commits marked text by inserting it; that copy stays local.
    if composing {
      super.insertText(insertString, replacementRange: replacementRange)
      return
    }
    let composed = takeMarkedText()
    let targets = composed.map { [NSRange(location: $0.location, length: 0)] }
    guard let inserted = plainString(insertString),
      inputDelegate?.insert(inserted, ranges: targets ?? ranges(replacementRange)) == true
    else {
      if composed == nil { super.insertText(insertString, replacementRange: replacementRange) }
      return
    }
  }

  override func setMarkedText(
    _ string: Any, selectedRange: NSRange, replacementRange: NSRange
  ) {
    if !hasMarkedText() {
      // A composition that starts over a selection first removes it in the engine.
      let selected = selectedRanges.map(\.rangeValue).filter { $0.length > 0 }
      if !selected.isEmpty { _ = inputDelegate?.delete(selected, backward: true) }
    }
    let wasComposing = composing
    composing = true
    defer { composing = wasComposing }
    super.setMarkedText(string, selectedRange: selectedRange, replacementRange: replacementRange)
  }

  override func unmarkText() {
    // AppKit re-enters here while a commit is already taking the marked text.
    guard !composing, let composed = takeMarkedText() else {
      super.unmarkText()
      return
    }
    if !composed.text.isEmpty {
      _ = inputDelegate?.insert(
        composed.text, ranges: [NSRange(location: composed.location, length: 0)])
    }
  }

  /// Paste, cut, drag and drop, and find-panel replacement reach the storage
  /// only through here. They become engine edits, or are refused.
  override func shouldChangeText(
    inRanges affectedRanges: [NSValue], replacementStrings: [String]?
  ) -> Bool {
    guard !composing, let replacementStrings else {
      return super.shouldChangeText(
        inRanges: affectedRanges, replacementStrings: replacementStrings)
    }
    _ = inputDelegate?.replaceVerbatim(
      affectedRanges.map(\.rangeValue), with: replacementStrings)
    return false
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

  @objc func undo(_ sender: Any?) {
    inputDelegate?.replayHistory(.undo)
  }

  @objc func redo(_ sender: Any?) {
    inputDelegate?.replayHistory(.redo)
  }

  /// Edit > Find menu items reach the engine-backed find bar, never
  /// AppKit's panel, which would search only the loaded page.
  override func performFindPanelAction(_ sender: Any?) {
    performFind(tag: (sender as? NSValidatedUserInterfaceItem)?.tag ?? 1)
  }

  override func performTextFinderAction(_ sender: Any?) {
    performFind(tag: (sender as? NSValidatedUserInterfaceItem)?.tag ?? 1)
  }

  private func performFind(tag: Int) {
    switch NSTextFinder.Action(rawValue: tag) {
    case .nextMatch: inputDelegate?.findNext(backward: false)
    case .previousMatch: inputDelegate?.findNext(backward: true)
    case .showReplaceInterface, .replace, .replaceAll, .replaceAndFind:
      inputDelegate?.presentFind(replace: true)
    default: inputDelegate?.presentFind(replace: false)
    }
  }

  override func validateUserInterfaceItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
    switch item.action {
    case #selector(undo(_:)), #selector(redo(_:)), #selector(performFindPanelAction(_:)),
      #selector(performTextFinderAction(_:)):
      inputDelegate != nil
    default: super.validateUserInterfaceItem(item)
    }
  }

  override func editorKeyEquivalent(with event: NSEvent) -> Bool {
    let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let key = event.charactersIgnoringModifiers?.lowercased()
    guard flags.contains(.command), window?.firstResponder === self else {
      return super.editorKeyEquivalent(with: event)
    }
    switch key {
    case "d": inputDelegate?.selectNextOccurrence(selectedRanges.map(\.rangeValue))
    case "/": inputDelegate?.toggleComment(selectedRanges.map(\.rangeValue))
    case "z": inputDelegate?.replayHistory(flags.contains(.shift) ? .redo : .undo)
    // Cmd+Shift+F stays Search Project; Cmd+Option+F opens find with replace.
    case "f" where !flags.contains(.shift):
      inputDelegate?.presentFind(replace: flags.contains(.option))
    case "g": inputDelegate?.findNext(backward: flags.contains(.shift))
    case "s": break
    default: return super.editorKeyEquivalent(with: event)
    }
    return true
  }

  override func keyDown(with event: NSEvent) {
    #if SOYEHT_PERFORMANCE_METER
      onPerformanceKeyDownStart?()
      defer { onPerformanceKeyDownEnd?() }
    #endif
    if event.keyCode == 53, inputDelegate?.closeFind() == true { return }
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
    guard event.modifierFlags.contains(.option) else {
      super.mouseDown(with: event)
      return
    }
    inputDelegate?.addCursor(at: characterOffset(for: event))
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

  /// Ends the composition and removes its marked text from the storage only;
  /// the engine never saw it. Returns where it was and what it said.
  private func takeMarkedText() -> (location: Int, text: String)? {
    guard hasMarkedText() else { return nil }
    let marked = markedRange()
    let text = (string as NSString).substring(with: marked)
    let wasComposing = composing
    composing = true
    defer { composing = wasComposing }
    super.unmarkText()
    textStorage?.replaceCharacters(in: marked, with: "")
    setSelectedRange(NSRange(location: marked.location, length: 0))
    return (marked.location, text)
  }

  private func plainString(_ value: Any) -> String? {
    (value as? String) ?? (value as? NSAttributedString)?.string
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
