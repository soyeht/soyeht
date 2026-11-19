#if os(macOS)
  import Foundation
  import Testing
  import SoyehtProtocol
  import SwiftTerm
  @testable import SoyehtClient

  private final class Mute: TerminalDelegate {
    func send(source: Terminal, data: ArraySlice<UInt8>) {}
  }

  /// What a Mac terminal surface holds of its pane: the end of the screen. It parses the bytes in
  /// SwiftTerm on the main actor as the surface does, which is what makes a connection with twelve
  /// long replays slower to read than the engine is to write.
  @MainActor private final class Surface {
    let paneId: String
    /// The last numbered line and the end marker that the pane's program printed.
    let lastLine: String
    let end: String
    private var window = Data()
    private let parser: Terminal?

    init(paneId: String, lastLine: String, end: String, parses: Bool = true) {
      (self.paneId, self.lastLine, self.end) = (paneId, lastLine, end)
      parser = parses ? Terminal(delegate: Mute(), options: .soyehtStartup) : nil
    }

    func receive(_ bytes: Data, reset: Bool) {
      if reset {
        window.removeAll()
        parser?.resetToInitialState()
      }
      parser?.feed(byteArray: Array(bytes))
      window.append(bytes)
      if window.count > 16384 { window = Data(window.suffix(16384)) }
    }

    var text: String { String(decoding: window, as: UTF8.self) }
    var holdsHistory: Bool { text.contains(lastLine) && text.contains(end) }
  }

  /// Twelve terminals in two workspaces, each holding more output than the engine keeps for a
  /// reconnecting client, and a client that connects as the Mac app does: the terminals of the
  /// selected workspace are on screen at launch, the others are shown later. The engine queues up
  /// to `tailBytes` of replay for every subscription and closes a connection that passes
  /// `connectionQueueBytes`, so a client that subscribes every terminal at once is closed as a slow
  /// consumer and reconnects for ever.
  @MainActor @Test func manyLongTerminalsConnectOnceAndKeepTheirHistory() async throws {
    guard let fixture = EngineFixture() else { return }
    let creator = EngineClient(socket: fixture.socketPath)
    let app = EngineClient(socket: fixture.socketPath)
    defer { fixture.tearDown(creator, app) }
    try await fixture.start()

    let lines = ProtocolLimits.tailBytes * 5 / 4 / 64
    var surfaces: [Surface] = []
    var workspaces: [String] = []
    creator.start()
    try await until { creator.connected }
    let creatorScreen = Surface(paneId: "", lastLine: "", end: "", parses: false)
    creator.terminalOutput = { bytes, reset in creatorScreen.receive(bytes, reset: reset) }
    // The workspace made last holds the focus the app restores, so it is the one on screen.
    for workspace in ["later", "shown"] {
      let folder = fixture.directory.appendingPathComponent(workspace)
      try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
      var workspaceId: String?
      for _ in 0..<6 {
        creatorScreen.receive(Data(), reset: true)
        let paneId = try await creator.createTerminalPane(
          directory: folder, workspaceId: workspaceId)
        workspaceId = creator.selectedPane?.workspaceId
        try await until { creatorScreen.text.contains("SWIFT_READY>") }
        let number = surfaces.count + 1
        let dots = String(repeating: ".", count: 64 - 13)
        creator.input(
          Data(
            ("awk -v t=\(number) -v n=\(lines) 'BEGIN{for(i=1;i<=n;i++)"
              + "printf \"T%02d %06d \(dots)\\n\",t,i; printf \"END_%02d\\n\",t}'\r").utf8))
        let surface = Surface(
          paneId: paneId, lastLine: String(format: "T%02d %06d ", number, lines),
          end: String(format: "END_%02d", number))
        try await until(seconds: 60) { creatorScreen.text.contains(surface.end) }
        surfaces.append(surface)
      }
      workspaces.append(try #require(workspaceId))
    }
    creator.stop()
    let (later, shown) = (Array(surfaces[..<6]), Array(surfaces[6...]))

    // A surface claims its pane's output and asks for the screen, as `MacTerminal` does.
    func mount(_ surface: Surface) {
      app.claimTerminalOutput(paneId: surface.paneId, owner: surface) { [surface] bytes, reset in
        surface.receive(bytes, reset: reset)
      }
      app.refreshTerminal(surface.paneId)
    }
    func hold(_ group: [Surface], _ what: String) async -> Bool {
      let held = await settles(seconds: 40) { group.allSatisfy(\.holdsHistory) }
      if !held {
        Issue.record("\(what): no history in \(group.filter { !$0.holdsHistory }.map(\.paneId))")
      }
      return held
    }
    func expectOneConnection(_ revision: UInt64, _ what: String) {
      #expect(app.connectionRevision == revision, "\(what): connections made")
      let closed = fixture.logText.split(separator: "\n").filter {
        $0.contains("engine.connection_closed") && $0.contains("slow_consumer")
      }
      #expect(closed.count == 0, "\(what): connections closed as slow consumers")
    }

    shown.forEach(mount)
    app.start()
    try await until(seconds: 60) { app.connected }
    #expect(app.selectedPane?.workspaceId == workspaces[1], "the app starts on the last workspace")
    _ = await hold(shown, "the terminals on screen at launch")
    later.forEach(mount)
    _ = await hold(later, "the terminals shown after the connection settled")
    expectOneConnection(1, "launch")

    let last = try #require(later.last)
    app.select(last.paneId)
    app.input(Data("printf 'TYPED_%s\\n' IN_LAST\r".utf8))
    #expect(await settles(seconds: 40) { last.text.contains("TYPED_IN_LAST") })

    for surface in surfaces { surface.receive(Data(), reset: true) }
    app.stop()
    app.start()
    #expect(await settles(seconds: 60) { app.connected && app.connectionRevision >= 2 })
    _ = await hold(surfaces, "after the reconnect")
    expectOneConnection(2, "reconnect")
  }

  /// Whether `condition` held before the deadline; a wait, never an assertion on elapsed time.
  @MainActor private func settles(seconds: Int, _ condition: () -> Bool) async -> Bool {
    do {
      try await until(seconds: seconds, condition)
      return true
    } catch {
      return false  // The caller records what it was waiting for.
    }
  }
#endif
