#if os(iOS)
  import SwiftUI
  import SwiftTerm
  import UIKit

  /// A paired device drives the terminal like the Mac does. SwiftTerm turns the software
  /// keyboard, its Esc/Ctrl/Tab/arrow accessory row and hardware keys (Return, Backspace,
  /// Tab, arrows, Ctrl-C, Ctrl-D, Esc) into terminal bytes, and `TerminalHost` sends them
  /// with the same `pane.input` path as the Mac terminal.
  @MainActor public struct PhoneTerminal: UIViewRepresentable {
    let client: EngineClient
    public init(client: EngineClient) { self.client = client }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(client: client, acceptsInput: true)
    }
    public func makeUIView(context: Context) -> TerminalView {
      let view = TerminalView(
        frame: .zero, font: .monospacedSystemFont(ofSize: 12, weight: .regular))
      applyTheme(to: view)
      view.accessibilityLabel = "Terminal"
      context.coordinator.attach(view)
      return view
    }
    public func updateUIView(_ view: TerminalView, context: Context) {
      applyTheme(to: view)
      context.coordinator.update(view)
    }

    private func applyTheme(to view: TerminalView) {
      let colors = SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette)
      view.nativeBackgroundColor = UIColor(colors.face)
      view.nativeForegroundColor = UIColor(colors.ink)
      view.setNeedsDisplay()
    }
    public static func dismantleUIView(_ view: TerminalView, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }
#endif
