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
      let background = TerminalHost.background
      view.nativeBackgroundColor = UIColor(
        red: background.red, green: background.green, blue: background.blue, alpha: 1)
      let foreground = TerminalHost.foreground
      view.nativeForegroundColor = UIColor(
        red: foreground.red, green: foreground.green, blue: foreground.blue, alpha: 1)
      view.accessibilityLabel = "Terminal screen, read-only"
      context.coordinator.attach(view)
      return view
    }
    public func updateUIView(_ view: TerminalView, context: Context) {
      context.coordinator.update(view)
    }
    public static func dismantleUIView(_ view: TerminalView, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }
#endif
