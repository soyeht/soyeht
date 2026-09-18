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
    let host = NSHostingView(rootView: MacTerminal(client: client, paneId: "pane-a"))
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

  @MainActor @Test func terminalSurfaceIdentityLivesWithItsPane() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let first = client.terminalViewRegistry.surface(for: "pane-a") {
      TerminalSurface(frame: .zero)
    }
    let reused = client.terminalViewRegistry.surface(for: "pane-a") {
      TerminalSurface(frame: .zero)
    }
    let sibling = client.terminalViewRegistry.surface(for: "pane-b") {
      TerminalSurface(frame: .zero)
    }

    #expect(first.isNew)
    #expect(!reused.isNew)
    #expect(first.view === reused.view)
    #expect(first.view !== sibling.view)

    client.terminalViewRegistry.retainOnly(["pane-b"])
    #expect(client.terminalViewRegistry.identity(for: "pane-a") == nil)
    #expect(
      client.terminalViewRegistry.identity(for: "pane-b") == ObjectIdentifier(sibling.view))

    let recreated = client.terminalViewRegistry.surface(for: "pane-a") {
      TerminalSurface(frame: .zero)
    }
    #expect(recreated.isNew)
    #expect(first.view !== recreated.view)
  }

  @MainActor @Test func terminalLifecycleUpdateDoesNotStealFieldFocus() async throws {
    _ = NSApplication.shared
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 800, height: 400),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    defer { window.close() }
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let host = NSHostingView(rootView: MacTerminal(client: client, paneId: "pane-a"))
    let field = NSTextField(frame: NSRect(x: 20, y: 20, width: 300, height: 30))
    window.contentView = host
    host.addSubview(field)
    host.layoutSubtreeIfNeeded()
    try await Task.sleep(for: .milliseconds(100))
    func terminal(in view: NSView) -> TerminalView? {
      if let terminal = view as? TerminalView { return terminal }
      return view.subviews.lazy.compactMap { terminal(in: $0) }.first
    }
    let view = try #require(terminal(in: host))
    #expect(window.makeFirstResponder(field))
    #expect(window.firstResponder is NSTextView)

    view.viewDidMoveToWindow()

    #expect(window.firstResponder is NSTextView)
  }

  @MainActor @Test func fieldKeepsEveryKeyAcrossTerminalLifecycleUpdate() async throws {
    _ = NSApplication.shared
    let window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 800, height: 400),
      styleMask: [.titled, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    defer { window.close() }
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let host = NSHostingView(rootView: MacTerminal(client: client, paneId: "pane-a"))
    let field = NSTextField(frame: NSRect(x: 20, y: 20, width: 300, height: 30))
    window.contentView = host
    host.addSubview(field)
    host.layoutSubtreeIfNeeded()
    try await Task.sleep(for: .milliseconds(100))
    func terminal(in view: NSView) -> TerminalView? {
      if let terminal = view as? TerminalView { return terminal }
      return view.subviews.lazy.compactMap { terminal(in: $0) }.first
    }
    let view = try #require(terminal(in: host))
    #expect(window.makeFirstResponder(field))
    view.viewDidMoveToWindow()

    let text = "controle-foco"
    for (index, character) in text.enumerated() {
      let event = try #require(
        NSEvent.keyEvent(
          with: .keyDown, location: .zero, modifierFlags: [],
          timestamp: Double(index), windowNumber: window.windowNumber, context: nil,
          characters: String(character), charactersIgnoringModifiers: String(character),
          isARepeat: false, keyCode: 0))
      window.sendEvent(event)
    }

    #expect(field.stringValue == text)
  }

  @MainActor @Test func disabledTerminalDropsKeyboardInputUntilReenabled() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    var forwarded: [Data] = []
    let host = TerminalHost(
      client: client, acceptsInput: true, sendInput: { forwarded.append($0) })
    let view = TerminalView(frame: .zero)
    let bytes = Array("echo unsafe\n".utf8)

    host.send(source: view, data: bytes[...])
    host.updateAcceptsInput(false)
    host.send(source: view, data: bytes[...])
    host.updateAcceptsInput(true)
    host.send(source: view, data: bytes[...])

    #expect(forwarded == [Data(bytes), Data(bytes)])
  }

  @MainActor @Test func simultaneousTerminalHostsRouteInputAndResizeToTheirOwnPane() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    var firstInput: [Data] = []
    var secondInput: [Data] = []
    var resizes: [(String?, Int, Int)] = []
    let first = TerminalHost(
      client: client, paneId: "pane-a", acceptsInput: false,
      sendInput: { firstInput.append($0) },
      resizeTerminal: { resizes.append(($0, $1, $2)) })
    let second = TerminalHost(
      client: client, paneId: "pane-b", acceptsInput: true,
      sendInput: { secondInput.append($0) },
      resizeTerminal: { resizes.append(($0, $1, $2)) })
    let firstView = TerminalView(frame: .zero)
    let secondView = TerminalView(frame: .zero)
    let bytes = Array("pwd\n".utf8)

    first.send(source: firstView, data: bytes[...])
    second.send(source: secondView, data: bytes[...])
    first.sizeChanged(source: firstView, newCols: 80, newRows: 24)
    second.sizeChanged(source: secondView, newCols: 120, newRows: 42)

    #expect(firstInput.isEmpty)
    #expect(secondInput == [Data(bytes)])
    #expect(resizes.map(\.0) == ["pane-a", "pane-b"])
    #expect(resizes.map(\.1) == [80, 120])
    #expect(resizes.map(\.2) == [24, 42])
  }

  @MainActor @Test func paneOutputSinksStayIsolatedAndLateReleaseCannotClearReplacement() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let firstOwner = NSObject()
    let replacementOwner = NSObject()
    let siblingOwner = NSObject()
    var first = Data()
    var replacement = Data()
    var sibling = Data()

    client.claimTerminalOutput(paneId: "pane-a", owner: firstOwner) { bytes, _ in
      first.append(bytes)
    }
    client.claimTerminalOutput(paneId: "pane-b", owner: siblingOwner) { bytes, _ in
      sibling.append(bytes)
    }
    client.deliverTerminalOutput(to: "pane-a", bytes: Data("old".utf8), reset: false)
    client.claimTerminalOutput(paneId: "pane-a", owner: replacementOwner) { bytes, _ in
      replacement.append(bytes)
    }
    client.releaseTerminalOutput(paneId: "pane-a", owner: firstOwner)
    client.deliverTerminalOutput(to: "pane-a", bytes: Data("new".utf8), reset: false)
    client.deliverTerminalOutput(to: "pane-b", bytes: Data("peer".utf8), reset: false)

    #expect(String(decoding: first, as: UTF8.self) == "old")
    #expect(String(decoding: replacement, as: UTF8.self) == "new")
    #expect(String(decoding: sibling, as: UTF8.self) == "peer")
    #expect(client.terminalOutputSinkCount == 2)
  }

  @MainActor @Test func terminalBodyClickRunsActivationBeforeNativeMouseHandling() throws {
    let surface = TerminalSurface(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
    var activated = false
    surface.updateActivation { activated = true }
    let event = try #require(
      NSEvent.mouseEvent(
        with: .leftMouseDown, location: NSPoint(x: 20, y: 20), modifierFlags: [],
        timestamp: 0, windowNumber: 0, context: nil, eventNumber: 1, clickCount: 1,
        pressure: 1))

    surface.mouseDown(with: event)

    #expect(activated)
  }

  @MainActor @Test func retiringTerminalCannotDetachItsReplacementOutputSink() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let first = TerminalHost(client: client, acceptsInput: true)
    let replacement = TerminalHost(client: client, acceptsInput: true)
    let view = TerminalView(frame: .zero)

    first.attach(view)
    replacement.attach(view)
    first.detach(view)

    #expect(view.terminalDelegate === replacement)
    #expect(client.terminalOutput != nil)
    replacement.detach(view)
    #expect(client.terminalOutput == nil)
  }

  @MainActor @Test func terminalWithoutAnInstalledSinkCannotReleaseTheCurrentSink() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let current = TerminalHost(client: client, acceptsInput: true)
    let neverAttached = TerminalHost(client: client, acceptsInput: true)
    let currentView = TerminalView(frame: .zero)

    current.attach(currentView)
    neverAttached.detach(TerminalView(frame: .zero))

    #expect(client.terminalOutput != nil)
    current.detach(currentView)
    #expect(client.terminalOutput == nil)
  }

  @MainActor @Test func replacementCanClaimOutputAfterPriorOwnerDisappears() {
    let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
    let abandonedView = TerminalView(frame: .zero)
    weak var abandoned: TerminalHost?
    do {
      let host = TerminalHost(client: client, acceptsInput: true)
      abandoned = host
      host.attach(abandonedView)
    }
    #expect(abandoned == nil)

    let replacement = TerminalHost(client: client, acceptsInput: true)
    let replacementView = TerminalView(frame: .zero)
    replacement.attach(replacementView)
    replacement.detach(replacementView)

    #expect(client.terminalOutput == nil)
  }
#endif
