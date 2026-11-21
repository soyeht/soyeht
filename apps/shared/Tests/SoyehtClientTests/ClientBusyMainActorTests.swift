#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  /// When each `method` call began, as the engine's log says (`engine.operation_started`).
  private func engineStarts(of method: String, in log: String) -> [Date] {
    let format = ISO8601DateFormatter()
    format.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return log.split(separator: "\n").compactMap { line in
      guard line.contains("\"engine.operation_started\""), line.contains("\"method\":\"\(method)\"")
      else { return nil }
      guard let range = line.range(of: "\"startedAt\":\"") else { return nil }
      let rest = line[range.upperBound...]
      return rest.firstIndex(of: "\"").flatMap { format.date(from: String(rest[..<$0])) }
    }
  }

  /// The whole client against the engine programs of the build, with the main actor held for
  /// seconds: a request whose deadline ends meanwhile is answered and the connection stays, and
  /// the keys typed meanwhile reach the engine at once, not when the main actor is free again.
  extension BusyMainActor {
    @Test func theClientKeepsItsConnectionAndItsKeysMovingWhileTheMainActorIsBusy() async throws {
      guard let fixture = EngineFixture() else { return }
      let client = EngineClient(socket: fixture.socketPath)
      defer { fixture.tearDown(client) }
      try await fixture.start()
      client.start()
      try await until(seconds: 90) { client.connected }
      var screen = Data()
      client.terminalOutput = { bytes, reset in
        if reset { screen.removeAll() }
        screen.append(bytes)
      }
      _ = try await client.createTerminalPane(directory: fixture.directory)
      try await until(seconds: 90) {
        String(decoding: screen, as: UTF8.self).contains("SWIFT_READY>")
      }
      let revision = client.connectionRevision

      let usage = Task {
        try await client.request(.usageList, .encoded(UsageListParams()), timeoutSeconds: 0.6)
      }
      // Forty keys, one after another, as typed; the engine takes each before the next leaves.
      let keys = Array("printf 'KEYS_%s' DONE; echo\r".utf8)
      for key in keys { client.input(Data([key])) }
      let heldAt = Date()
      await keepMainActorBusy(for: 0.8)
      let log = fixture.logText
      let typed = engineStarts(of: RequestMethod.paneInput.rawValue, in: log).filter {
        $0 >= heldAt - 1
      }
      // Keys typed while a piece is on its way go together, so there are fewer requests than keys.
      #expect(!typed.isEmpty)
      for start in typed { #expect(start < heldAt + 0.5, "A key reached the engine late") }
      let asked = engineStarts(of: RequestMethod.usageList.rawValue, in: log).filter {
        $0 >= heldAt - 1
      }
      #expect(!asked.isEmpty)

      _ = try await usage.value
      #expect(client.connected)
      #expect(client.connectionRevision == revision)
      try await until { String(decoding: screen, as: UTF8.self).contains("KEYS_DONE") }
    }
  }
#endif
