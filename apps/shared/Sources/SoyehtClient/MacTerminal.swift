import SwiftUI

/// The two typefaces of the approved pane interior: Inter for the interface and JetBrains Mono
/// for code. The terminal uses the same mono family, so code and terminal stay one face.
public enum PaneInteriorFont {
  public static let uiFamily = DesignTokens.Fonts.ui
  public static let monoFamily = DesignTokens.Fonts.mono

  public static func ui(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom(uiFamily, size: size).weight(weight)
  }

  public static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom(monoFamily, size: size).weight(weight)
  }
}

#if os(macOS)
  import AppKit
  import SwiftTerm

  private struct TerminalKeyboardInputEnabledKey: EnvironmentKey {
    static let defaultValue = true
  }

  extension EnvironmentValues {
    public var terminalKeyboardInputEnabled: Bool {
      get { self[TerminalKeyboardInputEnabledKey.self] }
      set { self[TerminalKeyboardInputEnabledKey.self] = newValue }
    }
  }

  public enum MacTerminalFont {
    public static let familyName = PaneInteriorFont.monoFamily
    public static let pointSize = CGFloat(PaneLayoutContentMetrics.terminalFontPointSize)

    public static func active() -> NSFont {
      guard let font = NSFont(name: familyName, size: pointSize) else {
        assertionFailure("Missing bundled terminal font: \(familyName)")
        return NSFont.monospacedSystemFont(ofSize: pointSize, weight: .regular)
      }
      return font
    }
  }

  @MainActor
  final class TerminalSurface: TerminalView {
    private var keyboardInputEnabled = true
    private var claimedInitialFocus = false
    private var onActivate: (() -> Void)?
    private(set) var isReceivingOutput = false
    private var appliedTheme: [SwiftUI.Color] = []

    private lazy var scrollerFade = TerminalScrollerFade(terminal: self)

    /// A surface with the clients' shared startup options (`TerminalOptions.soyehtStartup`).
    convenience init(font: NSFont) {
      self.init(frame: .zero, font: font, options: .soyehtStartup)
    }

    func updateActivation(_ action: @escaping () -> Void) { onActivate = action }

    /// Paints the surface in the theme's colors. Setting a color makes the emulator drop its
    /// attribute caches and mark every row dirty, so a surface that is updated for another
    /// reason (SwiftUI updates it whenever the pane card changes) keeps the colors it has.
    func applyTheme(_ colors: SoyehtThemeColorSet) {
      let theme = [colors.face, colors.ink]
      guard theme != appliedTheme else { return }
      appliedTheme = theme
      nativeBackgroundColor = NSColor(colors.face)
      nativeForegroundColor = NSColor(colors.ink)
      needsDisplay = true
    }

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      scrollerFade.attach()
      claimInitialFocusIfNeeded()
    }

    /// The display moved: show the scroller while the view is away from the live bottom.
    override func scrolled(source terminal: Terminal, yDisp: Int) {
      super.scrolled(source: terminal, yDisp: yDisp)
      scrollerFade.displayScrolled(awayFromBottom: canScroll && scrollPosition < 1)
    }

    /// The output of the pane, joined into one feed per display frame.
    private lazy var output = TerminalOutputCoalescer { [weak self] batch in
      self?.feed(batch)
    }

    func receive(_ bytes: Data, reset: Bool) { output.enqueue(bytes, reset: reset) }

    /// Feeds what waits, before something that must see the screen as it is: a new grid.
    func flushOutput() { output.flush() }

    /// The person typed: the echo of the key is not held for the frame.
    func noteUserInput() { output.noteInput() }

    private func feed(_ batch: TerminalOutputBatch) {
      isReceivingOutput = true
      defer { isReceivingOutput = false }
      if batch.reset { getTerminal().resetToInitialState() }
      feed(byteArray: batch.bytes[...])
    }

    func updateKeyboardInputEnabled(_ enabled: Bool) {
      guard keyboardInputEnabled != enabled else { return }
      keyboardInputEnabled = enabled
      guard let window else { return }
      if enabled {
        claimedInitialFocus = window.makeFirstResponder(self)
      } else if window.firstResponder === self {
        window.makeFirstResponder(nil)
      }
    }

    private func claimInitialFocusIfNeeded() {
      guard let window else {
        claimedInitialFocus = false
        return
      }
      guard keyboardInputEnabled, !claimedInitialFocus else { return }
      claimedInitialFocus = window.makeFirstResponder(self)
    }
    override func mouseDown(with event: NSEvent) {
      onActivate?()
      window?.makeFirstResponder(self)
      super.mouseDown(with: event)
    }
  }

  /// Keeps a terminal surface alive for exactly as long as its pane exists.
  ///
  /// SwiftUI may rebuild the representable when a pane tree is reparented (for example, when an
  /// earlier sibling closes). Reusing the AppKit surface preserves SwiftTerm's scrollback and
  /// viewport across that structural change.
  @MainActor
  final class MacTerminalViewRegistry {
    private var surfaces: [String: TerminalSurface] = [:]

    func surface(
      for paneId: String, make: () -> TerminalSurface
    ) -> (view: TerminalSurface, isNew: Bool) {
      if let surface = surfaces[paneId] { return (surface, false) }
      let surface = make()
      surfaces[paneId] = surface
      return (surface, true)
    }

    func remove(_ paneId: String) { surfaces.removeValue(forKey: paneId) }

    func retainOnly(_ paneIds: Set<String>) {
      surfaces = surfaces.filter { paneIds.contains($0.key) }
    }

    func identity(for paneId: String) -> ObjectIdentifier? {
      surfaces[paneId].map(ObjectIdentifier.init)
    }
  }

  @MainActor
  public struct MacTerminal: NSViewRepresentable {
    private let client: EngineClient
    private let paneId: String
    @Environment(\.terminalKeyboardInputEnabled) private var keyboardInputEnabled
    @Environment(\.filePathOpenAction) private var openFile
    @Environment(\.webLinkOpenAction) private var openWeb
    public init(client: EngineClient, paneId: String) {
      self.client = client
      self.paneId = paneId
    }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(
        client: client, paneId: paneId, acceptsInput: keyboardInputEnabled,
        ringBell: { NSSound.beep() },
        openFile: { workspaceId, reference in
          openFile(workspaceId: workspaceId, reference: reference)
        },
        openWeb: { workspaceId, url in
          openWeb(workspaceId: workspaceId, url: url)
        })
    }
    public func makeNSView(context: Context) -> TerminalView {
      let registered = client.terminalViewRegistry.surface(for: paneId) {
        TerminalSurface(font: MacTerminalFont.active())
      }
      let view = registered.view
      applyTheme(to: view)
      view.linkReporting = .implicit
      view.linkHighlightMode = .hover
      view.setAccessibilityLabel("Terminal pane")
      view.updateActivation { [weak client] in client?.select(paneId) }
      view.updateKeyboardInputEnabled(keyboardInputEnabled)
      client.claimTerminalOutput(paneId: paneId, owner: view) { [weak view] bytes, reset in
        view?.receive(bytes, reset: reset)
      }
      context.coordinator.attach(view, refresh: registered.isNew)
      return view
    }
    public func updateNSView(_ view: TerminalView, context: Context) {
      applyTheme(to: view)
      context.coordinator.updateAcceptsInput(keyboardInputEnabled)
      if let surface = view as? TerminalSurface {
        surface.updateActivation { [weak client] in client?.select(paneId) }
        surface.updateKeyboardInputEnabled(keyboardInputEnabled)
      }
      context.coordinator.updateOpenFile { workspaceId, reference in
        openFile(workspaceId: workspaceId, reference: reference)
      }
      context.coordinator.updateOpenWeb { workspaceId, url in
        openWeb(workspaceId: workspaceId, url: url)
      }
      context.coordinator.update(view)
    }

    private func applyTheme(to view: TerminalView) {
      (view as? TerminalSurface)?.applyTheme(
        SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette))
    }
    public static func dismantleNSView(_ view: TerminalView, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }
#endif
