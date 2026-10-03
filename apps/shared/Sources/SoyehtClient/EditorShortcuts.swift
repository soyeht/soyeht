/// Editor key commands that are not text. Each one carries Command: AppKit offers a
/// Command key event to the window's views and then to the main menu as a key equivalent
/// before any text view interprets it as typing. An Option-only chord is a character on most
/// layouts (Option+Z types "Ω" on a US keyboard), so the focused text view would insert it
/// and the command would never run.
public struct EditorShortcut: Equatable, Sendable {
  /// The key as `charactersIgnoringModifiers` reports it, lowercased.
  public let key: String
  public let shift: Bool
  public let option: Bool
  public let control: Bool

  public init(key: String, shift: Bool = false, option: Bool = false, control: Bool = false) {
    self.key = key
    self.shift = shift
    self.option = option
    self.control = control
  }

  /// View > Soft Wrap: Cmd+Option+Z. Zed binds the chord Cmd+K Z, which a menu item cannot
  /// hold, and VS Code binds Option+Z, which types "Ω". Adding Command keeps VS Code's key
  /// while making it a key equivalent. No other Soyeht menu, editor, find bar or tab key uses
  /// it; plain Cmd+Z and Cmd+Shift+Z stay undo and redo.
  public static let softWrap = EditorShortcut(key: "z", option: true)

  /// Toggle Project Panel: Cmd+Shift+E, Zed's project panel key.
  public static let projectPanel = EditorShortcut(key: "e", shift: true)
}

#if os(macOS)
  import AppKit

  extension EditorShortcut {
    public var modifierFlags: NSEvent.ModifierFlags {
      var flags: NSEvent.ModifierFlags = [.command]
      if shift { flags.insert(.shift) }
      if option { flags.insert(.option) }
      if control { flags.insert(.control) }
      return flags
    }

    /// True for a key event with exactly this key and these modifiers.
    public func matches(_ event: NSEvent) -> Bool {
      guard event.type == .keyDown else { return false }
      let flags = event.modifierFlags.intersection([.command, .shift, .option, .control])
      return flags == modifierFlags && event.charactersIgnoringModifiers?.lowercased() == key
    }
  }

  /// The editor's text view: it runs the editor's own key commands before AppKit or a
  /// subclass sees the event, so Cmd+Option+Z toggles soft wrap instead of reaching undo or
  /// the text. Subclasses override `editorKeyEquivalent(with:)`, not `performKeyEquivalent`.
  @MainActor open class EditorKeyCommandTextView: NSTextView {
    public var onToggleSoftWrap: (() -> Void)?

    public final override func performKeyEquivalent(with event: NSEvent) -> Bool {
      if window?.firstResponder === self, EditorShortcut.softWrap.matches(event),
        let onToggleSoftWrap
      {
        onToggleSoftWrap()
        return true
      }
      return editorKeyEquivalent(with: event)
    }

    open func editorKeyEquivalent(with event: NSEvent) -> Bool {
      super.performKeyEquivalent(with: event)
    }
  }
#endif
