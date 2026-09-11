#if os(macOS)
  import AppKit
  import SwiftUI
  import SwiftTerm
  import Testing
  @testable import SoyehtClient

  @MainActor @Test func terminalOwnsKeyboardFocusAfterAttachment() async throws {
    _ = NSApplication.shared
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 800, height: 400),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    defer { window.close() }
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let host = NSHostingView(rootView: MacTerminal(client: client))
    window.contentView = host
    host.layoutSubtreeIfNeeded()
    try await Task.sleep(for: .milliseconds(100))
    func terminal(in view: NSView) -> TerminalView? {
      if let terminal = view as? TerminalView { return terminal }
      return view.subviews.lazy.compactMap { terminal(in: $0) }.first
    }
    let view = try #require(terminal(in: host))
    #expect(window.firstResponder === view)
    let cols = view.getTerminal().cols
    window.setContentSize(NSSize(width: 1000, height: 400))
    host.layoutSubtreeIfNeeded()
    try await Task.sleep(for: .milliseconds(100))
    #expect(view.getTerminal().cols > cols)
  }
#endif
