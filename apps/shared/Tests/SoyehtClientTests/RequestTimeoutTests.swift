import SoyehtProtocol
import Testing
@testable import SoyehtClient

private actor TimeoutGate {
  private var released: Set<RequestMethod> = []
  private var waiters: [RequestMethod: CheckedContinuation<Void, Never>] = [:]

  func wait(for method: RequestMethod) async {
    if released.remove(method) != nil { return }
    await withCheckedContinuation { waiters[method] = $0 }
  }

  func release(_ method: RequestMethod) {
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
    method: RequestMethod, gate: TimeoutGate,
    registered: AsyncStream<RequestMethod>.Continuation
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
    first: RequestMethod, second: RequestMethod, victim: RequestMethod
  ) async -> (RequestMethod, String) {
    let gate = TimeoutGate()
    let (registrations, registered) = AsyncStream.makeStream(of: RequestMethod.self)
    var registration = registrations.makeAsyncIterator()
    let (completions, completed) = AsyncStream.makeStream(of: (RequestMethod, String).self)
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
    first: .harnessList, second: .modelList, victim: .modelList)
  #expect(modelVictim.0 == .modelList)
  #expect(modelVictim.1 == "The engine did not respond to model.list in time.")

  let harnessVictim = await firstTimeout(
    first: .harnessList, second: .modelList, victim: .harnessList)
  #expect(harnessVictim.0 == .harnessList)
  #expect(harnessVictim.1 == "The engine did not respond to harness.list in time.")
}
