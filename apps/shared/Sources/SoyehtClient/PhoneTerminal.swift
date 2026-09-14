#if os(iOS)
  import SwiftUI
  import SwiftTerm
  import UIKit

  @MainActor private final class ReadOnlyTerminal: TerminalView {
    override var canBecomeFirstResponder: Bool { false }
  }

  @MainActor public struct PhoneTerminal: UIViewRepresentable {
    let client: EngineClient
    public init(client: EngineClient) { self.client = client }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(client: client, acceptsInput: false)
    }
    public func makeUIView(context: Context) -> TerminalView {
      let view = ReadOnlyTerminal(
        frame: .zero, font: .monospacedSystemFont(ofSize: 12, weight: .regular))
      applyTheme(to: view)
      view.accessibilityLabel = "Terminal screen, read-only"
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
