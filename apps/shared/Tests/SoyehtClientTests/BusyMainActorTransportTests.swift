#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  /// A wire to the fake engine, its channel and a reader, with the greeting already read.
  @MainActor struct Link {
    let server: FakeEngineServer
    let wire: Wire
    let channel: RequestChannel
    let queue: IncomingQueue
    let reader: Task<Void, Never>

    init(
      answer: @escaping FakeEngineServer.Answer = { _ in .object([:]) },
      queueBytes: Int = ClientPolicy.capsIncomingQueueBytes,
      channel: RequestChannel = RequestChannel()
    ) async throws {
      self.channel = channel
      server = try FakeEngineServer(answer: answer)
      wire = Wire(socket: server.path)
      try await wire.open(greetingTimeout: .seconds(60))
      _ = try await wire.next()
      channel.attach(wire)
      let (wire, pending, queue) = (wire, channel.pending, IncomingQueue(limitBytes: queueBytes))
      self.queue = queue
      reader = Task.detached { await EngineReader.run(wire: wire, pending: pending, queue: queue) }
    }

    func stop() {
      reader.cancel()
      queue.finish()
      wire.close()
      server.stop()
    }

    /// What the main actor does with the answers the reader queues: resumes their callers.
    func applyingAnswers() -> Task<Void, Never> {
      let queue = queue
      return Task.detached {
        while true {
          let batch = await queue.next()
          if batch.isEmpty { return }
          for case .reply(let entry, _) in batch {
            entry.continuation.resume(returning: EngineResponse(value: .null, attachment: nil))
          }
        }
      }
    }

    func answer(_ id: String) throws {
      try server.push(json: [
        .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])])
      ])
    }
  }

  /// A deadline the test ends: every wait a request makes for its deadline lasts until `release`.
  actor Deadlines {
    private var open = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func wait() async {
      if open { return }
      await withCheckedContinuation { waiters.append($0) }
    }

    func release() {
      open = true
      for waiter in waiters { waiter.resume() }
      waiters.removeAll()
    }
  }

  /// Deadlines that pass one by one, when the test says: each `tick` ends one wait.
  actor Ticks {
    private var spare = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func wait() async {
      if spare > 0 {
        spare -= 1
        return
      }
      await withCheckedContinuation { waiters.append($0) }
    }

    func tick() {
      if waiters.isEmpty { spare += 1 } else { waiters.removeFirst().resume() }
    }
  }

  private actor Count {
    private(set) var value = 0
    func add() { value += 1 }
  }

  private actor PieceLog {
    private(set) var values: [Data] = []
    var count: Int { values.count }
    var total: Int { values.reduce(0) { $0 + $1.count } }
    func add(_ value: Data) { values.append(value) }
  }

  /// A link to an engine that answers everything except `usage.list`.
  @MainActor private func linkHoldingUsageList() async throws -> Link {
    try await Link(answer: { $0.method == RequestMethod.usageList.rawValue ? nil : .object([:]) })
  }

  /// A link to an engine that takes at most `limit` bytes of each `pane.input`.
  @MainActor private func linkAccepting(upTo limit: Int) async throws -> Link {
    try await Link(answer: { request in
      let count = request.params["bytes"].string.flatMap { Data(base64Encoded: $0)?.count } ?? 0
      return .object(["acceptedBytes": .integer(Int64(min(count, limit)))])
    })
  }

  /// A `usage.list` request the engine has read and left unanswered, and its id.
  @MainActor private func unansweredRequest(
    on link: Link,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void =
      PendingRequests.waitForTimeout
  ) async throws -> (task: Task<EngineResponse, Error>, id: String) {
    let channel = link.channel
    let task = Task.detached {
      try await channel.response(
        .usageList, .object([:]), timeoutSeconds: 30, waitForTimeout: waitForTimeout)
    }
    while link.server.requests.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
    return (task, try #require(link.server.requests.first?.id))
  }

  /// A request the engine has read and will not answer, with deadlines the test ends (`ticks`),
  /// on a link whose answers to everything else are applied.
  @MainActor private func slowRequestBehindOthers() async throws -> (
    link: Link, applier: Task<Void, Never>, slow: Task<EngineResponse, Error>, id: String,
    ticks: Ticks
  ) {
    let link = try await linkHoldingUsageList()
    let ticks = Ticks()
    let (slow, id) = try await unansweredRequest(
      on: link, waitForTimeout: { _, _ in await ticks.wait() })
    return (link, link.applyingAnswers(), slow, id, ticks)
  }

  /// A `usage.list` request whose deadline passes when `deadlines` say so.
  private func requestHeld(by deadlines: Deadlines, on channel: RequestChannel) -> Task<
    EngineResponse, Error
  > {
    Task.detached {
      try await channel.response(
        .usageList, .object([:]), timeoutSeconds: 12,
        waitForTimeout: { _, _ in await deadlines.wait() })
    }
  }

  private func pieces(of server: FakeEngineServer) -> [Data] {
    server.requests.filter { $0.method == RequestMethod.paneInput.rawValue }.compactMap {
      $0.params["bytes"].string.flatMap { Data(base64Encoded: $0) }
    }
  }

  /// The transport, against an engine that speaks only the frames, with the main actor busy where
  /// the test says so. Nothing here reads the clock to decide a result: a deadline passes when the
  /// test ends it, and a wait ends when what it waits for has happened.
  @Suite(.serialized) @MainActor struct BusyMainActor {
    @Test func anAnswerReadWhileTheMainActorIsBusyIsNeverATimeout() async throws {
      let link = try await Link()
      defer { link.stop() }
      let channel = link.channel
      let deadlines = Deadlines()
      let request = requestHeld(by: deadlines, on: channel)
      // The main actor is busy until the answer has been read: nothing applied it.
      await keepMainActorBusy(until: { !link.server.requests.isEmpty && channel.pending.count == 0 }
      )
      #expect(channel.pending.count == 0)
      // Now the deadline passes. The answer had already left its reach.
      await deadlines.release()
      try await Task.sleep(for: .milliseconds(100))
      #expect(link.wire.isOpen)
      var applied = false
      while !applied {
        for case .reply(let entry, let message) in await link.queue.next() {
          entry.continuation.resume(
            returning: EngineResponse(value: message["result"], attachment: nil))
          applied = true
        }
      }
      #expect(try await request.value.value == .object([:]))
    }

    @Test func anEngineThatNeverAnswersStillFailsTheRequestAndClosesTheConnection() async throws {
      let link = try await Link(answer: { _ in nil })
      defer { link.stop() }
      do {
        // The deadline passes at once, and nothing was answered meanwhile.
        _ = try await link.channel.response(
          .usageList, .object([:]), timeoutSeconds: 5, waitForTimeout: { _, _ in })
        Issue.record("A silent engine answered")
      } catch ClientFailure.requestTimeout(let method) {
        #expect(method == RequestMethod.usageList.rawValue)
      }
      #expect(!link.wire.isOpen)
    }

    @Test func aRequestWaitingBehindOthersWaitsAnotherWindowWhileTheEngineKeepsAnswering()
      async throws
    {
      let (link, applier, slow, id, ticks) = try await slowRequestBehindOthers()
      defer { link.stop() }
      defer { applier.cancel() }
      let channel = link.channel
      // The engine answers another request, then the first window of the slow one passes.
      _ = try await channel.response(.harnessList, .object([:]), timeoutSeconds: 30)
      await ticks.tick()
      // It waits another window, and the engine answers it.
      try link.answer(id)
      _ = try await slow.value
      #expect(link.wire.isOpen)
    }

    @Test func aRequestTheEngineNeverAnswersFailsAloneWhileItAnswersTheOthers() async throws {
      let (link, applier, slow, _, ticks) = try await slowRequestBehindOthers()
      defer { link.stop() }
      defer { applier.cancel() }
      let channel = link.channel
      for _ in 0..<ClientPolicy.capsRequestWindows {
        _ = try await channel.response(.harnessList, .object([:]), timeoutSeconds: 30)
        await ticks.tick()
      }
      do {
        _ = try await slow.value
        Issue.record("An unanswered request completed")
      } catch ClientFailure.requestTimeout(let method) {
        #expect(method == RequestMethod.usageList.rawValue)
      }
      // It failed alone: the engine was answering, so the connection stays.
      #expect(link.wire.isOpen)
    }

    @Test func answersAndEventsReachTheMainActorInTheOrderTheyWereRead() async throws {
      let link = try await Link(answer: { _ in nil })
      defer { link.stop() }
      let (first, id) = try await unansweredRequest(on: link)
      func event(_ n: Int) -> JsonValue {
        .object([
          "jsonrpc": .string("2.0"), "method": .string("test.event"), "params": .integer(Int64(n)),
        ])
      }
      try link.server.push(
        json: (0..<50).map(event) + [
          .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])])
        ] + (50..<100).map(event))
      await keepMainActorBusy(until: { link.queue.queuedBytes > 0 })
      var seen: [String] = []
      while seen.count < 101 {
        for item in await link.queue.next() {
          switch item {
          case .message(let message): seen.append(String(message["params"].int ?? -1))
          case .reply(let entry, _):
            seen.append("reply")
            entry.continuation.resume(returning: EngineResponse(value: .null, attachment: nil))
          default: break
          }
        }
      }
      var expected: [String] = (0..<50).map { String($0) }
      expected.append("reply")
      expected += (50..<100).map { String($0) }
      #expect(seen == expected)
      _ = try await first.value
    }

    @Test func aBacklogOverTheLimitFailsTheRequestsItDropsAndEndsTheConnection() async throws {
      let link = try await Link(answer: { _ in nil }, queueBytes: 400)
      defer { link.stop() }
      let (request, id) = try await unansweredRequest(on: link)
      // The answer, then more than the queue holds, none of it applied.
      var frames: [JsonValue] = [
        .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])])
      ]
      for n in 0..<30 {
        frames.append(
          .object([
            "jsonrpc": .string("2.0"), "method": .string("test.event"),
            "params": .integer(Int64(n)),
          ]))
      }
      try link.server.push(json: frames)
      do {
        _ = try await request.value
        Issue.record("A request whose answer was dropped completed")
      } catch ClientFailure.invalidStream {
      }
      #expect(!link.wire.isOpen)
      #expect(link.queue.failure is ClientFailure)
      #expect(await link.queue.next().isEmpty)
    }

    @Test func terminalInputLeavesInOrderWhileTheMainActorIsBusy() async throws {
      let link = try await linkAccepting(upTo: .max)
      defer { link.stop() }
      let input = TerminalInputQueue.sending(
        through: link.channel, limitBytes: 1 << 20, chunkBytes: 4, isConnected: { true },
        failed: { Issue.record("Input failed: \($0.localizedDescription)") })
      let text = "echo the quick brown fox\r"
      for character in text { #expect(input.enqueue(Data(String(character).utf8), paneId: "p")) }
      // The main actor stays busy until the engine has every key: they left meanwhile.
      let server = link.server
      await keepMainActorBusy(until: {
        pieces(of: server).reduce(0) { $0 + $1.count } >= text.utf8.count
      })
      while !input.isIdle { try await Task.sleep(for: .milliseconds(5)) }
      let typed = String(
        decoding: pieces(of: server).reduce(into: Data()) { $0.append($1) }, as: UTF8.self)
      #expect(typed == text)
    }

    @Test func keysTypedWhileAPieceIsOnItsWayGoTogetherInTheNextOne() async throws {
      let gate = AsyncStream.makeStream(of: Void.self)
      let log = PieceLog()
      let input = TerminalInputQueue(
        limitBytes: 1 << 20, chunkBytes: 8,
        send: { _, chunk in
          await log.add(chunk)
          if await log.count == 1 { for await _ in gate.stream { break } }
          return chunk.count
        },
        failed: { _ in })
      #expect(input.enqueue(Data("a".utf8), paneId: "p"))
      while await log.count < 1 { await Task.yield() }
      // The first piece is out and its answer has not come: these wait.
      for key in "bcdefghijkl" { #expect(input.enqueue(Data(String(key).utf8), paneId: "p")) }
      gate.continuation.yield()
      while await log.total < 12 { await Task.yield() }
      let sent = await log.values.map { String(decoding: $0, as: UTF8.self) }
      #expect(sent == ["a", "bcdefghi", "jkl"])
    }

    @Test func inputTheEngineTakesOnlyPartOfIsResentInOrderAndInChunks() async throws {
      let link = try await linkAccepting(upTo: 3)
      defer { link.stop() }
      let input = TerminalInputQueue.sending(
        through: link.channel, limitBytes: 1 << 20, chunkBytes: 8, isConnected: { true },
        failed: { Issue.record("Input failed: \($0.localizedDescription)") })
      let first = Data("abcdefghijklmnopqrst".utf8)
      let second = Data("UVWXYZ".utf8)
      #expect(input.enqueue(first, paneId: "p"))
      #expect(input.enqueue(second, paneId: "p"))
      let server = link.server
      await keepMainActorBusy(until: {
        pieces(of: server).reduce(0) { $0 + min($1.count, 3) } >= 26
      })
      while !input.isIdle { try await Task.sleep(for: .milliseconds(5)) }
      let bytes = pieces(of: server)
      #expect(bytes.allSatisfy { $0.count <= 8 })
      // The engine kept 3 bytes of each piece: what it kept, in order, is what was typed.
      let kept = bytes.reduce(into: Data()) { $0.append($1.prefix(3)) }
      #expect(kept == first + second)
    }

    @Test func inputIsSentAgainWithTheSameKeyAfterTheConnectionIsLost() async throws {
      // Two connections to two engines, one channel; the one attached last is the one in use.
      let channel = RequestChannel()
      let second = try await Link(
        answer: { _ in .object(["acceptedBytes": .integer(1)]) }, channel: channel)
      let first = try await Link(answer: { _ in nil }, channel: channel)
      defer { second.stop() }
      let secondWire = second.wire
      let input = TerminalInputQueue.sending(
        through: channel,
        isConnected: {
          // The client reconnected: a new wire.
          channel.attach(secondWire)
          return true
        },
        failed: { Issue.record("Input failed: \($0.localizedDescription)") })
      #expect(input.enqueue(Data("x".utf8), paneId: "p"))
      let firstServer = first.server
      while firstServer.requests.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
      // The connection is lost with the key in flight.
      channel.pending.failAll(ClientFailure.disconnected)
      let secondServer = second.server
      while secondServer.requests.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
      while !input.isIdle { try await Task.sleep(for: .milliseconds(5)) }
      let keys = [first.server, second.server].compactMap { server in
        server.requests.first?.params["idempotencyKey"].string
      }
      #expect(keys.count == 2)
      #expect(Set(keys).count == 1)
      first.stop()
    }

    @Test func inputBeyondTheQueueLimitIsRefusedWholeAndNothingIsQueued() async throws {
      let held = AsyncStream.makeStream(of: Void.self)
      let input = TerminalInputQueue(
        limitBytes: 10, chunkBytes: 4,
        send: { _, chunk in
          for await _ in held.stream { break }
          return chunk.count
        },
        failed: { _ in })
      #expect(input.enqueue(Data(repeating: 1, count: 6), paneId: "p"))
      #expect(!input.enqueue(Data(repeating: 2, count: 20), paneId: "p"))
      held.continuation.finish()
    }

    @Test func aKeyDoesNotWaitBehindABurstOfTheClientsOwnRequests() async throws {
      // An engine that serves one request at a time and takes 20 ms for each, as under load.
      let link = try await Link(answer: { _ in
        Thread.sleep(forTimeInterval: 0.02)
        return .object(["acceptedBytes": .integer(1)])
      })
      defer { link.stop() }
      let channel = link.channel
      let applier = link.applyingAnswers()
      defer { applier.cancel() }
      let finished = Count()
      let burst = (0..<100).map { _ in
        Task.detached {
          _ = try? await channel.response(.harnessList, .object([:]), timeoutSeconds: 30)
          await finished.add()
        }
      }
      while link.server.requests.count < 2 { try await Task.sleep(for: .milliseconds(2)) }
      _ = try await channel.response(
        .paneInput, .object(["paneId": .string("p"), "bytes": .string("YQ==")]),
        timeoutSeconds: 30, delivery: .immediate, lane: .priority)
      // Behind at most the requests already sent, not behind the hundred.
      #expect(await finished.value < 50)
      for task in burst { await task.value }
    }

    @Test func aLongLineOfRequestsOnAClosedWireAllFailAndNoneOverflowsTheStack() async throws {
      // Two requests hold the only places and are never answered; thousands wait behind them.
      let link = try await Link(answer: { _ in nil })
      defer { link.stop() }
      let channel = link.channel
      let deadlines = Deadlines()
      let holders = (0..<ClientPolicy.capsRequestsInFlight).map { _ in
        requestHeld(by: deadlines, on: channel)
      }
      let server = link.server
      while server.requests.count < holders.count { try await Task.sleep(for: .milliseconds(2)) }
      let line = 5000
      let waiters = (0..<line).map { _ in
        Task.detached { () -> Bool in
          do {
            _ = try await channel.response(.harnessList, .object([:]), timeoutSeconds: 600)
            return false
          } catch ClientFailure.disconnected {
            return true
          } catch {
            return false
          }
        }
      }
      while channel.waitingForTheEngine < line { try await Task.sleep(for: .milliseconds(5)) }
      // The wire is closed and the client has not yet noticed; then a holder's deadline passes and
      // each place goes to the next in line, whose start is refused at once.
      link.wire.close()
      await deadlines.release()
      var failed = 0
      for waiter in waiters where await waiter.value { failed += 1 }
      #expect(failed == line)
      for holder in holders { _ = try? await holder.value }
    }

    @Test func requestsMadeInOrderInOneTurnReachTheEngineInThatOrder() async throws {
      let link = try await Link()
      defer { link.stop() }
      let applier = link.applyingAnswers()
      defer { applier.cancel() }
      let channel = link.channel
      let rounds = 100
      for _ in 0..<rounds {
        let a = Task { @MainActor in
          _ = try? await channel.response(.usageList, .object([:]), timeoutSeconds: 30)
        }
        let b = Task { @MainActor in
          _ = try? await channel.response(.harnessList, .object([:]), timeoutSeconds: 30)
        }
        await a.value
        await b.value
      }
      let methods = link.server.requests.map(\.method)
      let expected = Array(
        repeating: [RequestMethod.usageList.rawValue, RequestMethod.harnessList.rawValue],
        count: rounds
      )
      .flatMap { $0 }
      #expect(methods == expected)
    }
  }
#endif
