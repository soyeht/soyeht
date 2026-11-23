#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  /// How many times the engine's log says it began `method`.
  private func engineStarts(of method: String, in log: String) -> Int {
    log.split(separator: "\n").filter {
      $0.contains("\"engine.operation_started\"") && $0.contains("\"method\":\"\(method)\"")
    }.count
  }

  /// What the shown terminal has printed so far.
  @MainActor private final class Screen {
    var text = ""
  }

  /// Keys and a request with a deadline of a second, with the main actor busy for longer: both
  /// reach the engine meanwhile and the request is answered, not timed out.
  @MainActor private func keysAndARequestWhileBusy(
    _ client: EngineClient, _ fixture: EngineFixture, _ screen: Screen
  ) async throws {
    let revision = client.connectionRevision
    let (inputs, usages) = (
      engineStarts(of: RequestMethod.paneInput.rawValue, in: fixture.logText),
      engineStarts(of: RequestMethod.usageList.rawValue, in: fixture.logText)
    )
    let usage = Task {
      try await client.request(.usageList, .encoded(UsageListParams()), timeoutSeconds: 1)
    }
    for key in "printf 'KEYS_%s' DONE; echo\r".utf8 { client.input(Data([key])) }
    await keepMainActorBusy(
      for: 1.5,
      until: {
        let log = fixture.logText
        return engineStarts(of: RequestMethod.paneInput.rawValue, in: log) > inputs
          && engineStarts(of: RequestMethod.usageList.rawValue, in: log) > usages
      })
    _ = try await usage.value
    #expect(client.connected)
    #expect(client.connectionRevision == revision)
    try await until(seconds: 90) { screen.text.contains("KEYS_DONE") }
  }

  /// A backlog over its limit ends the connection, says why, and the next connection starts again
  /// from the engine's state.
  @MainActor private func aBacklogOverItsLimitEndsTheConnectionAndSaysWhy(
    _ client: EngineClient
  ) async throws {
    try await until(seconds: 90) { client.connected }
    client.incomingQueueBytes = 256 * 1024
    client.stop()
    client.start()
    try await until(seconds: 90) { client.connected }
    let before = client.connectionRevision
    client.input(Data("head -c 3000000 /dev/urandom | base64\r".utf8))
    let resynchronize = ClientFailure.invalidStream.localizedDescription
    await keepMainActorBusy(until: { client.connectionText == resynchronize }, cap: 60)
    #expect(client.connectionText == resynchronize)
    #expect(!client.connected)
    client.incomingQueueBytes = ClientPolicy.capsIncomingQueueBytes
    try await until(seconds: 90) { client.connected && client.connectionRevision > before }
  }

  /// The whole client against the engine programs of the build, with the main actor busy: a
  /// request whose deadline passes meanwhile is answered and the connection stays, the keys typed
  /// meanwhile reach the engine, and a backlog over its limit ends the connection and says why.
  /// One engine and one client for all of it. No result here is a time: a wait ends when what it
  /// waits for has happened. (The order of answers and frames is `ApplyOrder`'s, the order of
  /// requests on the wire `BusyMainActor`'s.)
  extension BusyMainActor {
    @Test func theClientKeepsItsConnectionAndItsKeysMovingWhileTheMainActorIsBusy() async throws {
      guard let fixture = EngineFixture() else { return }
      let client = EngineClient(socket: fixture.socketPath)
      defer { fixture.tearDown(client) }
      try await fixture.start()
      client.start()
      try await until(seconds: 90) { client.connected }
      let screen = Screen()
      client.terminalOutput = { bytes, reset in
        if reset { screen.text = "" }
        screen.text += String(decoding: bytes, as: UTF8.self)
      }
      _ = try await client.createTerminalPane(directory: fixture.directory)
      try await until(seconds: 90) { screen.text.contains("SWIFT_READY>") }
      try await keysAndARequestWhileBusy(client, fixture, screen)
      try await aBacklogOverItsLimitEndsTheConnectionAndSaysWhy(client)
    }
  }
#endif
