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

  /// The methods of the `engine.operation_started` lines of `log`, in the order the engine wrote
  /// them.
  private func startedMethods(in log: Substring) -> [String] {
    log.split(separator: "\n").compactMap { line in
      guard line.contains("\"engine.operation_started\""),
        let range = line.range(of: "\"method\":\"")
      else { return nil }
      let rest = line[range.upperBound...]
      return rest.firstIndex(of: "\"").map { String(rest[..<$0]) }
    }
  }

  private final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false
    var isSet: Bool { lock.withLock { value } }
    func set() { lock.withLock { value = true } }
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

  /// A caller resumes before the output that follows its answer is applied. A round in which the
  /// answer is slower than the output it is compared with (a loaded host) proves nothing and is
  /// made again; a caller that is late is late in every round.
  @MainActor private func callersResumeBeforeTheOutputBehindTheirAnswer(
    _ client: EngineClient, _ screen: Screen
  ) async throws {
    for round in 0..<6 {
      var late = true
      for attempt in 0..<3 where late {
        let marker = "RESULT_\(round)_\(attempt)_"
        client.input(Data("sleep 0.05; printf 'RES%sULT_\(round)_\(attempt)_\\n' ''\r".utf8))
        let caller = Task { @MainActor () -> Bool in
          _ = try? await client.request(.usageList, .encoded(UsageListParams()))
          return screen.text.contains(marker)
        }
        // The request is on its way; the window is held while its answer and, 50 ms later, the
        // output come back, and both wait in the backlog.
        for _ in 0..<4 { await Task.yield() }
        holdMainActor(0.15)
        late = await caller.value
        try await until(seconds: 90) { screen.text.contains(marker) }
      }
      #expect(!late, "A caller resumed after the output behind its answer was applied")
    }
  }

  /// Two requests made one after the other in one turn reach the engine in that order.
  @MainActor private func requestsReachTheEngineInTheOrderTheyWereMade(
    _ client: EngineClient, _ fixture: EngineFixture
  ) async throws {
    let mark = fixture.logText.endIndex
    let pairs = 100
    let failed = Flag()
    for _ in 0..<pairs {
      let a = Task { @MainActor in
        do { _ = try await client.request(.usageList, .encoded(UsageListParams())) } catch {
          failed.set()
        }
      }
      let b = Task { @MainActor in
        do {
          _ = try await client.request(.harnessCustomList, .encoded(HarnessCustomListParams()))
        } catch { failed.set() }
      }
      await a.value
      await b.value
    }
    #expect(!failed.isSet)
    func pairOrder() -> [String] {
      startedMethods(in: fixture.logText[mark...]).filter {
        $0 == RequestMethod.usageList.rawValue || $0 == RequestMethod.harnessCustomList.rawValue
      }
    }
    // The engine's log is written a moment after the answers are sent.
    try await until(seconds: 60) { pairOrder().count >= 2 * pairs }
    let order = pairOrder()
    var swapped = 0
    for index in stride(from: 0, to: order.count - 1, by: 2)
    where order[index] == RequestMethod.harnessCustomList.rawValue {
      swapped += 1
    }
    #expect(order.count == 2 * pairs)
    #expect(swapped == 0, "A request made second reached the engine first")
  }

  /// A client stopped while an answer waits in its backlog ends the request it answers.
  @MainActor private func stoppingWithAnAnswerInTheBacklogEndsItsRequest(
    _ client: EngineClient
  ) async throws {
    for _ in 0..<5 {
      try await until(seconds: 90) { client.connected }
      let done = Flag()
      Task { @MainActor in
        _ = try? await client.request(.usageList, .encoded(UsageListParams()))
        done.set()
      }
      for _ in 0..<4 { await Task.yield() }
      // The answer is read and waits in the backlog while the window is held; then the client is
      // stopped, as an iPhone going to the background does.
      holdMainActor(0.2)
      client.stop()
      await keepMainActorBusy(until: { done.isSet }, cap: 10)
      #expect(done.isSet, "A request answered just before stop() never ended")
      client.start()
    }
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
  /// meanwhile reach the engine, a caller resumes before the frames behind its answer are applied,
  /// requests reach the engine in the order they were made, a client stopped with an answer in its
  /// backlog ends every request, and a backlog over its limit ends the connection and says why.
  /// One engine and one client for all of it. No result here is a time: a wait ends when what it
  /// waits for has happened.
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
      try await callersResumeBeforeTheOutputBehindTheirAnswer(client, screen)
      try await requestsReachTheEngineInTheOrderTheyWereMade(client, fixture)
      try await stoppingWithAnAnswerInTheBacklogEndsItsRequest(client)
      try await aBacklogOverItsLimitEndsTheConnectionAndSaysWhy(client)
    }
  }
#endif
