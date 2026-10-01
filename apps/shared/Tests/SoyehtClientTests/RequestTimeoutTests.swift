import Testing
@testable import SoyehtClient

private actor TimeoutGate {
  private var released: Set<String> = []
  private var waiters: [String: CheckedContinuation<Void, Never>] = [:]

  func wait(for method: String) async {
    if released.remove(method) != nil { return }
    await withCheckedContinuation { waiters[method] = $0 }
  }

  func release(_ method: String) {
    if let waiter = waiters.removeValue(forKey: method) {
      waiter.resume()
    } else {
      released.insert(method)
    }
  }
}

@MainActor
@Test func requestTimeoutNamesWhicheverMethodFailsToFinish() async {
  let client = EngineClient(socket: "/tmp/request-timeout-attribution.sock")
  func timeoutMessage(
    method: String, gate: TimeoutGate,
    registered: AsyncStream<String>.Continuation
  ) async -> String {
    do {
      _ = try await client.pendingResponse(
        method, .object([:]), timeoutSeconds: 12,
        waitForTimeout: { method, _ in await gate.wait(for: method) },
        send: { _ in registered.yield(method) })
      return "request unexpectedly completed"
    } catch {
      return error.localizedDescription
    }
  }
  func firstTimeout(
    first: String, second: String, victim: String
  ) async -> (String, String) {
    let gate = TimeoutGate()
    let (registrations, registered) = AsyncStream.makeStream(of: String.self)
    var registration = registrations.makeAsyncIterator()
    let (completions, completed) = AsyncStream.makeStream(of: (String, String).self)
    var completion = completions.makeAsyncIterator()
    let firstTask = Task {
      let message = await timeoutMessage(method: first, gate: gate, registered: registered)
      completed.yield((first, message))
    }
    #expect(await registration.next() == first)
    let secondTask = Task {
      let message = await timeoutMessage(method: second, gate: gate, registered: registered)
      completed.yield((second, message))
    }
    #expect(await registration.next() == second)
    await gate.release(victim)
    let observed = await completion.next()!
    await gate.release(victim == first ? second : first)
    _ = await completion.next()
    await firstTask.value
    await secondTask.value
    return observed
  }

  let modelVictim = await firstTimeout(
    first: "harness.list", second: "model.list", victim: "model.list")
  #expect(modelVictim.0 == "model.list")
  #expect(modelVictim.1 == "The engine did not respond to model.list in time.")

  let harnessVictim = await firstTimeout(
    first: "harness.list", second: "model.list", victim: "harness.list")
  #expect(harnessVictim.0 == "harness.list")
  #expect(harnessVictim.1 == "The engine did not respond to harness.list in time.")
}
