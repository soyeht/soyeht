#if os(macOS)
  import AppKit
  import SwiftUI
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

  @MainActor
  private final class TerminalSurface: TerminalView {
    private var keyboardInputEnabled = true
    private var claimedInitialFocus = false

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

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      guard let window else {
        claimedInitialFocus = false
        return
      }
      guard keyboardInputEnabled, !claimedInitialFocus else { return }
      claimedInitialFocus = window.makeFirstResponder(self)
    }
    override func mouseDown(with event: NSEvent) {
      if keyboardInputEnabled { window?.makeFirstResponder(self) }
      super.mouseDown(with: event)
    }
  }

  @MainActor
  public struct MacTerminal: NSViewRepresentable {
    private let client: EngineClient
    @Environment(\.terminalKeyboardInputEnabled) private var keyboardInputEnabled
    @Environment(\.filePathOpenAction) private var openFile
    @Environment(\.webLinkOpenAction) private var openWeb
    public init(client: EngineClient) { self.client = client }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(
        client: client, acceptsInput: keyboardInputEnabled, ringBell: { NSSound.beep() },
        openFile: { workspaceId, reference in
          openFile(workspaceId: workspaceId, reference: reference)
        },
        openWeb: { workspaceId, url in
          openWeb(workspaceId: workspaceId, url: url)
        })
    }
    public func makeNSView(context: Context) -> TerminalView {
      let view = TerminalSurface(
        frame: .zero,
        font: .monospacedSystemFont(
          ofSize: PaneLayoutContentMetrics.terminalFontPointSize, weight: .regular))
      applyTheme(to: view)
      view.linkReporting = .implicit
      view.linkHighlightMode = .hover
      view.setAccessibilityLabel("Selected pane terminal")
      view.updateKeyboardInputEnabled(keyboardInputEnabled)
      context.coordinator.attach(view)
      return view
    }
    public func updateNSView(_ view: TerminalView, context: Context) {
      applyTheme(to: view)
      context.coordinator.updateAcceptsInput(keyboardInputEnabled)
      (view as? TerminalSurface)?.updateKeyboardInputEnabled(keyboardInputEnabled)
      context.coordinator.updateOpenFile { workspaceId, reference in
        openFile(workspaceId: workspaceId, reference: reference)
      }
      context.coordinator.updateOpenWeb { workspaceId, url in
        openWeb(workspaceId: workspaceId, url: url)
      }
      context.coordinator.update(view)
    }

    private func applyTheme(to view: TerminalView) {
      let colors = SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette)
      view.nativeBackgroundColor = NSColor(colors.face)
      view.nativeForegroundColor = NSColor(colors.ink)
      view.needsDisplay = true
    }
    public static func dismantleNSView(_ view: TerminalView, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }
#endif
