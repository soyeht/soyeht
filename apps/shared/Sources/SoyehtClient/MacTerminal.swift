#if os(macOS)
  import AppKit
  import SwiftUI
  import SwiftTerm

  @MainActor
  private final class TerminalSurface: TerminalView {
    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      window?.makeFirstResponder(self)
    }
    override func mouseDown(with event: NSEvent) {
      window?.makeFirstResponder(self)
      super.mouseDown(with: event)
    }
  }

  @MainActor
  public struct MacTerminal: NSViewRepresentable {
    private let client: EngineClient
    public init(client: EngineClient) { self.client = client }
    public func makeCoordinator() -> TerminalHost {
      TerminalHost(client: client, acceptsInput: true) { NSSound.beep() }
    }
    public func makeNSView(context: Context) -> TerminalView {
      let view = TerminalSurface(
        frame: .zero, font: .monospacedSystemFont(ofSize: 13, weight: .regular))
      let background = TerminalHost.background
      view.nativeBackgroundColor = NSColor(
        red: background.red, green: background.green, blue: background.blue, alpha: 1)
      let foreground = TerminalHost.foreground
      view.nativeForegroundColor = NSColor(
        red: foreground.red, green: foreground.green, blue: foreground.blue, alpha: 1)
      view.setAccessibilityLabel("Selected pane terminal")
      context.coordinator.attach(view)
      return view
    }
    public func updateNSView(_ view: TerminalView, context: Context) {
      context.coordinator.update(view)
    }
    public static func dismantleNSView(_ view: TerminalView, coordinator: TerminalHost) {
      coordinator.detach(view)
    }
  }
#endif
