import Testing

@testable import SoyehtClient

@Test func editorShortcutsAllCarryCommandAndDoNotCollide() {
  #expect(EditorShortcut.softWrap != EditorShortcut.projectPanel)
  #expect(EditorShortcut.softWrap.option && !EditorShortcut.softWrap.shift)
}

#if os(macOS)
  import AppKit

  /// Delivers a key event in AppKit's order: a Command key event goes to the window's views
  /// and then the main menu as a key equivalent; whatever is left reaches the first responder
  /// as typing.
  @MainActor private func deliver(_ event: NSEvent, in window: NSWindow, menu: NSMenu) {
    if event.modifierFlags.contains(.command) {
      if window.performKeyEquivalent(with: event) { return }
      if menu.performKeyEquivalent(with: event) { return }
    }
    window.sendEvent(event)
  }

  @MainActor private func keyDown(
    _ characters: String, key: String, keyCode: UInt16, flags: NSEvent.ModifierFlags,
    window: NSWindow
  ) -> NSEvent {
    NSEvent.keyEvent(
      with: .keyDown, location: .zero, modifierFlags: flags, timestamp: 0,
      windowNumber: window.windowNumber, context: nil, characters: characters,
      charactersIgnoringModifiers: key, isARepeat: false, keyCode: keyCode)!
  }

  @MainActor private final class Counter: NSObject {
    var count = 0
    @objc func fire(_ sender: Any?) { count += 1 }
  }

  /// The View menu item, built from the same shortcut the Mac menu uses.
  @MainActor private func viewMenu(_ counter: Counter) -> NSMenu {
    let menu = NSMenu()
    let item = NSMenuItem(
      title: "Soft Wrap", action: #selector(Counter.fire(_:)),
      keyEquivalent: EditorShortcut.softWrap.key)
    item.keyEquivalentModifierMask = EditorShortcut.softWrap.modifierFlags
    item.target = counter
    menu.addItem(item)
    return menu
  }

  /// Cmd+Option+Z on a focused editor text view toggles soft wrap once and types nothing,
  /// while Option+Z, the shortcut it replaced, is text that reaches the file.
  @MainActor @Test func softWrapKeyTogglesInTheFocusedEditorAndTypesNothing() throws {
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled],
      backing: .buffered, defer: false)
    let textView = EditorKeyCommandTextView(frame: window.contentView!.bounds)
    window.contentView!.addSubview(textView)
    textView.string = "abc"
    var toggles = 0
    textView.onToggleSoftWrap = { toggles += 1 }
    let menuCounter = Counter()
    let menu = viewMenu(menuCounter)
    #expect(window.makeFirstResponder(textView))
    textView.setSelectedRange(NSRange(location: 3, length: 0))

    let softWrap = keyDown(
      "Ω", key: "z", keyCode: 6, flags: [.command, .option], window: window)
    #expect(EditorShortcut.softWrap.matches(softWrap))
    deliver(softWrap, in: window, menu: menu)
    #expect(toggles == 1)
    #expect(menuCounter.count == 0, "the editor handled it; the menu must not toggle again")
    #expect(textView.string == "abc")

    // Plain Cmd+Z is not soft wrap.
    #expect(
      !EditorShortcut.softWrap.matches(
        keyDown("z", key: "z", keyCode: 6, flags: [.command], window: window)))

    // The old Option+Z is a character: delivered the same way, it is typed.
    deliver(
      keyDown("Ω", key: "z", keyCode: 6, flags: [.option], window: window), in: window, menu: menu)
    #expect(toggles == 1)
    #expect(textView.string == "abcΩ")
  }

  /// With the find field focused, the menu item catches the same key and nothing is typed.
  @MainActor @Test func softWrapKeyReachesTheMenuFromTheFindField() throws {
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 400, height: 300), styleMask: [.titled],
      backing: .buffered, defer: false)
    let field = NSTextField(frame: NSRect(x: 10, y: 10, width: 200, height: 24))
    window.contentView!.addSubview(field)
    field.stringValue = "needle"
    let counter = Counter()
    let menu = viewMenu(counter)
    #expect(window.makeFirstResponder(field))

    deliver(
      keyDown("Ω", key: "z", keyCode: 6, flags: [.command, .option], window: window),
      in: window, menu: menu)
    #expect(counter.count == 1)
    #expect(field.currentEditor()?.string ?? field.stringValue == "needle")
  }
#endif
