import Testing

@testable import SoyehtClient

@Suite struct RequestSlotsTests {
  @Test func onlyAsManyAsTheLimitHoldASlotAndTheRestWaitInOrder() async throws {
    let slots = RequestSlots(2)
    try await slots.acquire()
    try await slots.acquire()
    let order = OrderLog()
    let waiters = (0..<3).map { n in
      Task {
        try await slots.acquire()
        await order.add(n)
      }
    }
    while slots.waiting < 3 { await Task.yield() }
    #expect(await order.values.isEmpty)
    slots.release()
    try await waiters[0].value
    slots.release()
    try await waiters[1].value
    slots.release()
    try await waiters[2].value
    #expect(await order.values == [0, 1, 2])
  }

  @Test func aWaiterThatIsCancelledHoldsNothingAndLeavesTheLine() async throws {
    let slots = RequestSlots(1)
    try await slots.acquire()
    let waiter = Task { try await slots.acquire() }
    while slots.waiting < 1 { await Task.yield() }
    waiter.cancel()
    await #expect(throws: CancellationError.self) { try await waiter.value }
    #expect(slots.waiting == 0)
    slots.release()
    try await slots.acquire()  // the slot the cancelled waiter never took is free
  }
}

private actor OrderLog {
  private(set) var values: [Int] = []
  func add(_ value: Int) { values.append(value) }
}
