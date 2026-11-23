#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  /// The client's apply loop, fed by hand: what a caller sees when its answer and the frames behind
  /// it are in the backlog together.
  @MainActor @Suite struct ApplyOrder {
    @MainActor private struct Setup {
      let client = EngineClient(socket: "")
      let server: FakeEngineServer
      let wire: Wire
      let queue = IncomingQueue()

      init() async throws {
        server = try FakeEngineServer(answer: { _ in nil })
        wire = Wire(socket: server.path)
        try await wire.open(greetingTimeout: .seconds(60))
        _ = try await wire.next()
        client.channel.attach(wire)
      }

      func stop() {
        wire.close()
        server.stop()
      }

      /// The id of the request the engine has read, and its answer taken as the reader takes it.
      func answered() async throws -> Incoming {
        while server.requests.isEmpty { try await Task.sleep(for: .milliseconds(5)) }
        let id = try #require(server.requests.first?.id)
        let entry = try #require(client.channel.pending.answer(id))
        return .reply(
          entry, .object(["jsonrpc": .string("2.0"), "id": .string(id), "result": .object([:])]))
      }

      /// A request is answered and an event follows its answer in the backlog; the apply loop
      /// (at `priority`) runs. Whether the caller, when it resumed, already saw the event.
      func callerSawTheEventBehindItsAnswer(priority: _Concurrency.TaskPriority?) async throws
        -> Bool
      {
        let client = client
        let caller = Task { @MainActor () -> Bool in
          _ = try await client.request(.usageList, .encoded(UsageListParams()))
          // What the answer and everything before it left.
          return client.reachability != nil
        }
        let reply = try await answered()
        let behind = Incoming.message(
          .object([
            "jsonrpc": .string("2.0"),
            "method": .string(EventName.engineReachabilityChanged.rawValue),
            "params": .object(["remote": .bool(true)]),
          ]))
        #expect(queue.push(reply))
        #expect(queue.push(behind))
        let (queue, wire) = (queue, wire)
        let applier = Task(priority: priority) { @MainActor in
          await client.apply(queue, from: wire)
        }
        let saw = try await caller.value
        try await until { client.reachability != nil }
        queue.finish()
        await applier.value
        return saw
      }
    }

    @Test func aCallerResumesBeforeTheFramesBehindItsAnswerAreApplied() async throws {
      let setup = try await Setup()
      defer { setup.stop() }
      #expect(try await setup.callerSawTheEventBehindItsAnswer(priority: nil) == false)
    }

    /// The caller goes first because the apply loop waits for it (`Acknowledgement`), not because
    /// both run at one priority: an apply loop of the lowest priority still applies the frame
    /// behind the answer after the caller.
    @Test func aCallerStillGoesFirstWhenTheApplyLoopHasTheLowestPriority() async throws {
      for _ in 0..<5 {
        let setup = try await Setup()
        defer { setup.stop() }
        #expect(try await setup.callerSawTheEventBehindItsAnswer(priority: .background) == false)
      }
    }

    @Test func anAnswerReadBeforeTheConnectionEndedStillEndsItsRequest() async throws {
      let setup = try await Setup()
      defer { setup.stop() }
      let client = setup.client
      let ended = Ended()
      Task { @MainActor in
        do { _ = try await client.request(.usageList, .encoded(UsageListParams())) } catch {
          ended.set(error)
        }
      }
      let reply = try await setup.answered()
      #expect(setup.queue.push(reply))
      // The client was stopped before the main actor reached the answer.
      client.channel.attach(nil)
      let applier = Task { @MainActor in await client.apply(setup.queue, from: setup.wire) }
      try await until { ended.error != nil }
      #expect(ended.error is ClientFailure)
      setup.queue.finish()
      await applier.value
    }
  }

  private final class Ended: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Error?
    var error: Error? { lock.withLock { stored } }
    func set(_ error: Error) { lock.withLock { stored = error } }
  }
#endif
