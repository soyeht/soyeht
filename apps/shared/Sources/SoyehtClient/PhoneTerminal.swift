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
    let claimsKeyboardFocus: Bool

    /// With `claimsKeyboardFocus` the terminal becomes first responder as soon as it is on
    /// screen, so a hardware keyboard types into it without a tap first.
    public init(client: EngineClient, claimsKeyboardFocus: Bool = false) {
      self.client = client
      self.claimsKeyboardFocus = claimsKeyboardFocus
    }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(client: client, acceptsInput: true)
    }
    public func makeUIView(context: Context) -> PhoneTerminalSurface {
      let view = PhoneTerminalSurface(
        frame: .zero, font: .monospacedSystemFont(ofSize: 12, weight: .regular))
      view.claimsKeyboardFocus = claimsKeyboardFocus
      applyTheme(to: view)
      view.accessibilityLabel = "Terminal"
      context.coordinator.attach(view)
      return view
    }
    public func updateUIView(_ view: PhoneTerminalSurface, context: Context) {
      view.claimsKeyboardFocus = claimsKeyboardFocus
      applyTheme(to: view)
      context.coordinator.update(view)
    }

    private func applyTheme(to view: TerminalView) {
      let colors = SoyehtThemeColorSet(palette: SoyehtThemeStore.shared.activeTheme.palette)
      view.nativeBackgroundColor = UIColor(colors.face)
      view.nativeForegroundColor = UIColor(colors.ink)
      view.setNeedsDisplay()
    }
    public static func dismantleUIView(_ view: PhoneTerminalSurface, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }

  public final class PhoneTerminalSurface: TerminalView {
    var claimsKeyboardFocus = false

    public override func didMoveToWindow() {
      super.didMoveToWindow()
      guard claimsKeyboardFocus, window != nil else { return }
      // SwiftUI inserts the view before its transition settles; claim focus on the next turn.
      DispatchQueue.main.async { [weak self] in
        guard let self, self.window != nil, !self.isFirstResponder else { return }
        _ = self.becomeFirstResponder()
      }
    }
  }
#endif
