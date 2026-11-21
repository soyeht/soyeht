#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  /// A wire to the fake engine, its channel and a reader, with the greeting already read.
  @MainActor private struct Link {
    let server: FakeEngineServer
    let wire: Wire
    let channel = RequestChannel()
    let queue: IncomingQueue
    let reader: Task<Void, Never>

    init(
      answer: @escaping FakeEngineServer.Answer = { _ in .object([:]) },
      queueBytes: Int = ClientPolicy.capsIncomingQueueBytes
    ) async throws {
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
  }

  /// What the main actor does with the answers the reader queues: resumes their callers.
  private func applyingAnswers(of link: Link) -> Task<Void, Never> {
    let queue = link.queue
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

  /// Other requests, answered by the fake engine every 40 ms for `seconds`.
  private func answeredTraffic(on channel: RequestChannel, for seconds: Double) -> Task<Void, Never>
  {
    Task.detached {
      let end = DispatchTime.now().uptimeNanoseconds + UInt64(seconds * 1_000_000_000)
      while DispatchTime.now().uptimeNanoseconds < end {
        _ = try? await channel.response(.harnessList, .object([:]), timeoutSeconds: 5)
        try? await Task.sleep(for: .milliseconds(40))
      }
    }
  }

  @MainActor
  @Test func aRequestWaitingBehindOthersIsNoTimeoutWhileTheEngineKeepsAnswering() async throws {
    let link = try await Link(answer: {
      $0.method == RequestMethod.usageList.rawValue ? nil : .object([:])
    })
    defer { link.stop() }
    let channel = link.channel
    let traffic = answeredTraffic(on: channel, for: 4.5)
    let applier = applyingAnswers(of: link)
    defer { applier.cancel() }
    let slow = Task.detached {
      try await channel.response(.usageList, .object([:]), timeoutSeconds: 1.0)
    }
    while link.server.requests.first(where: { $0.method == RequestMethod.usageList.rawValue })
      == nil
    {
      try await Task.sleep(for: .milliseconds(5))
    }
    let id = try #require(
      link.server.requests.first { $0.method == RequestMethod.usageList.rawValue }?.id)
    // Two and a half windows (of a second) pass with the engine answering others, then it answers this one.
    try await Task.sleep(for: .milliseconds(2500))
    try link.server.push(json: [
      .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])])
    ])
    _ = try await slow.value
    #expect(link.wire.isOpen)
    await traffic.value
  }

  @MainActor
  @Test func aRequestTheEngineNeverAnswersFailsAloneWhileItAnswersTheOthers() async throws {
    let link = try await Link(answer: {
      $0.method == RequestMethod.usageList.rawValue ? nil : .object([:])
    })
    defer { link.stop() }
    let channel = link.channel
    let traffic = answeredTraffic(on: channel, for: 3.5)
    let applier = applyingAnswers(of: link)
    defer { applier.cancel() }
    do {
      _ = try await channel.response(.usageList, .object([:]), timeoutSeconds: 0.6)
      Issue.record("An unanswered request completed")
    } catch ClientFailure.requestTimeout(let method) {
      #expect(method == RequestMethod.usageList.rawValue)
    }
    // It failed alone: the engine was answering, so the connection stays.
    #expect(link.wire.isOpen)
    await traffic.value
  }

  @MainActor
  @Test func aKeyDoesNotWaitBehindABurstOfTheClientsOwnRequests() async throws {
    // An engine that serves one request at a time and takes 20 ms for each, as under load.
    let link = try await Link(answer: { _ in
      Thread.sleep(forTimeInterval: 0.04)
      return .object(["acceptedBytes": .integer(1)])
    })
    defer { link.stop() }
    let channel = link.channel
    let applier = applyingAnswers(of: link)
    defer { applier.cancel() }
    let burst = (0..<100).map { _ in
      Task.detached {
        _ = try? await channel.response(.harnessList, .object([:]), timeoutSeconds: 10)
      }
    }
    try await Task.sleep(for: .milliseconds(100))
    let startedNs = DispatchTime.now().uptimeNanoseconds
    _ = try await channel.response(
      .paneInput, .object(["paneId": .string("p"), "bytes": .string("YQ==")]),
      timeoutSeconds: 5, delivery: .immediate)
    let waitedNs = DispatchTime.now().uptimeNanoseconds - startedNs
    // Behind at most the requests already sent (2 of them, 40 ms), not the 100 (4 s).
    #expect(waitedNs < 1_500_000_000)
    for task in burst { await task.value }
  }

  /// The tests that keep the main actor busy run one at a time: each one slows every other
  /// main-actor job of the suite by a slice per hop.
  @Suite(.serialized) @MainActor struct BusyMainActor {
    @Test func anAnswerReadWhileTheMainActorIsBusyIsNeverATimeout() async throws {
      let link = try await Link()
      defer { link.stop() }
      let channel = link.channel
      let request = Task.detached {
        try await channel.response(.usageList, .object([:]), timeoutSeconds: 0.75)
      }
      await keepMainActorBusy(for: 1.0)
      // The answer was read and taken out of the waiting requests while nothing applied it: its
      // deadline passed with nothing to fail, and the connection stayed open.
      #expect(link.channel.pending.count == 0)
      #expect(link.wire.isOpen)
      let batch = await link.queue.next()
      guard case .reply(let entry, let message)? = batch.first else {
        Issue.record("The answer was not queued for the main actor")
        return
      }
      entry.continuation.resume(
        returning: EngineResponse(value: message["result"], attachment: nil))
      #expect(try await request.value.value == .object([:]))
    }

    @Test func anEngineThatNeverAnswersStillFailsTheRequestAndClosesTheConnection() async throws {
      let link = try await Link(answer: { _ in nil })
      defer { link.stop() }
      do {
        _ = try await link.channel.response(.usageList, .object([:]), timeoutSeconds: 0.3)
        Issue.record("A silent engine answered")
      } catch ClientFailure.requestTimeout(let method) {
        #expect(method == RequestMethod.usageList.rawValue)
      }
      #expect(!link.wire.isOpen)
    }

    @Test func answersAndEventsReachTheMainActorInTheOrderTheyWereRead() async throws {
      let link = try await Link(answer: { _ in nil })
      defer { link.stop() }
      let channel = link.channel
      let first = Task.detached {
        try await channel.response(.usageList, .object([:]), timeoutSeconds: 30)
      }
      while link.server.requests.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
      let id = try #require(link.server.requests.first?.id)
      func event(_ n: Int) -> JsonValue {
        .object([
          "jsonrpc": .string("2.0"), "method": .string("test.event"), "params": .integer(Int64(n)),
        ])
      }
      try link.server.push(
        json: (0..<50).map(event) + [
          .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])])
        ] + (50..<100).map(event))
      await keepMainActorBusy(for: 0.3)
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

    @Test func terminalInputLeavesInOrderWhileTheMainActorIsBusy() async throws {
      let link = try await Link(answer: { request in
        let count = request.params["bytes"].string.flatMap { Data(base64Encoded: $0)?.count } ?? 0
        return .object(["acceptedBytes": .integer(Int64(count))])
      })
      defer { link.stop() }
      let channel = link.channel
      let input = TerminalInputQueue(
        limitBytes: 1 << 20, chunkBytes: 4,
        send: { paneId, chunk in
          let params = JsonValue.object([
            "paneId": .string(paneId), "bytes": .string(chunk.base64EncodedString()),
          ])
          let result = try await channel.response(
            .paneInput, params, timeoutSeconds: 5, delivery: .immediate)
          return result.value["acceptedBytes"].int ?? 0
        },
        failed: { Issue.record("Input failed: \($0.localizedDescription)") })
      let text = "echo the quick brown fox\r"
      let startedNs = DispatchTime.now().uptimeNanoseconds
      for character in text { #expect(input.enqueue(Data(String(character).utf8), paneId: "p")) }
      await keepMainActorBusy(for: 0.6)
      let sent = link.server.requests.filter { $0.method == RequestMethod.paneInput.rawValue }
      let received = sent.compactMap {
        $0.params["bytes"].string.flatMap { Data(base64Encoded: $0) }
      }
      .reduce(into: Data()) { $0.append($1) }
      // Every key reached the engine, in order, while the main actor was held.
      let typed = String(decoding: received, as: UTF8.self)
      #expect(typed == text)
      let blockEndNs = DispatchTime.now().uptimeNanoseconds
      let leftBy = blockEndNs - 200_000_000
      for request in sent { #expect(request.arrivedNs < leftBy) }
      let firstDelayNs = (sent.first?.arrivedNs ?? .max) &- startedNs
      #expect(firstDelayNs < 300_000_000)
    }

    @Test func inputTheEngineTakesOnlyPartOfIsResentInOrderAndInChunks() async throws {
      let link = try await Link(answer: { request in
        let count = request.params["bytes"].string.flatMap { Data(base64Encoded: $0)?.count } ?? 0
        return .object(["acceptedBytes": .integer(Int64(min(count, 3)))])
      })
      defer { link.stop() }
      let channel = link.channel
      let done = AsyncStream.makeStream(of: Void.self)
      let input = TerminalInputQueue(
        limitBytes: 1 << 20, chunkBytes: 8,
        send: { paneId, chunk in
          let params = JsonValue.object([
            "paneId": .string(paneId), "bytes": .string(chunk.base64EncodedString()),
          ])
          let result = try await channel.response(
            .paneInput, params, timeoutSeconds: 5, delivery: .immediate)
          let accepted = result.value["acceptedBytes"].int ?? 0
          if accepted == chunk.count { done.continuation.yield() }
          return accepted
        },
        failed: { Issue.record("Input failed: \($0.localizedDescription)") })
      let first = Data("abcdefghijklmnopqrst".utf8)
      let second = Data("UVWXYZ".utf8)
      #expect(input.enqueue(first, paneId: "p"))
      #expect(input.enqueue(second, paneId: "p"))
      var finished = 0
      for await _ in done.stream {
        finished += 1
        if finished == 2 { break }
      }
      let bytes = link.server.requests.compactMap {
        $0.params["bytes"].string.flatMap { Data(base64Encoded: $0) }
      }
      #expect(bytes.allSatisfy { $0.count <= 8 })
      // The engine kept 3 bytes of each piece: what it kept, in order, is what was typed.
      let kept = bytes.reduce(into: Data()) { $0.append($1.prefix(3)) }
      #expect(kept == first + second)
    }
  }

  @Test func keysTypedWhileAPieceIsOnItsWayGoTogetherInTheNextOne() async throws {
    let gate = AsyncStream.makeStream(of: Void.self)
    let pieces = PieceLog()
    let input = TerminalInputQueue(
      limitBytes: 1 << 20, chunkBytes: 8,
      send: { _, chunk in
        await pieces.add(chunk)
        if await pieces.count == 1 { for await _ in gate.stream { break } }
        return chunk.count
      },
      failed: { _ in })
    #expect(input.enqueue(Data("a".utf8), paneId: "p"))
    while await pieces.count < 1 { await Task.yield() }
    // The first piece is out and its answer has not come: these wait.
    for key in "bcdefghijkl" { #expect(input.enqueue(Data(String(key).utf8), paneId: "p")) }
    gate.continuation.yield()
    while await pieces.total < 12 { await Task.yield() }
    let sent = await pieces.values.map { String(decoding: $0, as: UTF8.self) }
    #expect(sent == ["a", "bcdefghi", "jkl"])
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

  @Test func aFullQueueFailsTheRequestsItDropsAndEndsTheConnection() async throws {
    let queue = IncomingQueue(limitBytes: 100)
    #expect(queue.push(.message(.null), bytes: 60))
    #expect(!queue.push(.message(.null), bytes: 60))
    queue.finish(failing: ClientFailure.invalidStream)
    #expect(await queue.next().isEmpty)
  }

  private actor PieceLog {
    private(set) var values: [Data] = []
    var count: Int { values.count }
    var total: Int { values.reduce(0) { $0 + $1.count } }
    func add(_ value: Data) { values.append(value) }
  }

#endif
