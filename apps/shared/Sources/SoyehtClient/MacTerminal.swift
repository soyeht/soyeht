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
    public func makeCoordinator() -> Coordinator { Coordinator(client: client) }
    public func makeNSView(context: Context) -> TerminalView {
      let view = TerminalSurface(
        frame: .zero, font: .monospacedSystemFont(ofSize: 13, weight: .regular))
      view.nativeBackgroundColor = NSColor(
        red: 232 / 255, green: 237 / 255, blue: 244 / 255, alpha: 1)
      view.nativeForegroundColor = NSColor(
        red: 62 / 255, green: 74 / 255, blue: 102 / 255, alpha: 1)
      view.terminalDelegate = context.coordinator
      view.setAccessibilityLabel("Selected pane terminal")
      let coordinator = context.coordinator
      client.terminalOutput = { [weak view, weak coordinator] bytes, reset in
        guard let view, let coordinator else { return }
        coordinator.feeding = true
        if reset { view.getTerminal().resetToInitialState() }
        view.feed(byteArray: Array(bytes)[...])
        coordinator.feeding = false
      }
      client.refreshTerminal()
      return view
    }
    public func updateNSView(_ view: TerminalView, context: Context) {
      guard client.connected, let pane = client.selectedPane else { return }
      let terminal = view.getTerminal()
      let revision = "\(pane.paneId):\(client.connectionRevision)"
      if context.coordinator.selection != revision {
        context.coordinator.selection = revision
        client.resize(cols: terminal.cols, rows: terminal.rows)
      } else if pane.cols != terminal.cols || pane.rows != terminal.rows {
        context.coordinator.applyingRemoteSize = true
        terminal.resize(cols: Int(pane.cols), rows: Int(pane.rows))
        context.coordinator.applyingRemoteSize = false
      }
    }
    public static func dismantleNSView(_ view: TerminalView, coordinator: Coordinator) {
      view.terminalDelegate = nil
      coordinator.client.terminalOutput = nil
    }
    @MainActor
    public final class Coordinator: NSObject, @preconcurrency TerminalViewDelegate {
      let client: EngineClient
      var feeding = false
      var selection: String?
      var applyingRemoteSize = false
      init(client: EngineClient) { self.client = client }
      public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        if !applyingRemoteSize { client.resize(cols: newCols, rows: newRows) }
      }
      public func send(source: TerminalView, data: ArraySlice<UInt8>) {
        // Replayed control sequences must not become new input to the shell.
        if !feeding { client.input(Data(data)) }
      }
      public func setTerminalTitle(source: TerminalView, title: String) {}
      public func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
      public func scrolled(source: TerminalView, position: Double) {}
      public func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {}
      public func bell(source: TerminalView) { NSSound.beep() }
      public func clipboardCopy(source: TerminalView, content: Data) {}
      public func clipboardRead(source: TerminalView) -> Data? { nil }
      public func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
      public func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
  }
#endif
