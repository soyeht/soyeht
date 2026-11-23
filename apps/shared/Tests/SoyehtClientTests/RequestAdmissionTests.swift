import Foundation
import Testing

@testable import SoyehtClient

private final class Started: @unchecked Sendable {
  private let lock = NSLock()
  private var ids: [String] = []
  var values: [String] { lock.withLock { ids } }
  func add(_ id: String) { lock.withLock { ids.append(id) } }
}

@Suite struct RequestAdmissionTests {
  @Test func onlyAsManyAsTheLimitStartAndTheRestStartInTheOrderTheyWereMade() {
    let admission = RequestAdmission(2)
    let started = Started()
    for id in ["a", "b", "c", "d"] { admission.enter(id: id, lane: .normal) { started.add(id) } }
    #expect(started.values == ["a", "b"])
    admission.finish("a")
    #expect(started.values == ["a", "b", "c"])
    admission.finish("b")
    admission.finish("c")
    #expect(started.values == ["a", "b", "c", "d"])
    admission.finish("d")
    // Every slot is free again: the next two start at once.
    admission.enter(id: "e", lane: .normal) { started.add("e") }
    admission.enter(id: "f", lane: .normal) { started.add("f") }
    #expect(started.values.suffix(2) == ["e", "f"])
  }

  @Test func aPriorityRequestStartsAtOnceAndTakesNoSlot() {
    let admission = RequestAdmission(1)
    let started = Started()
    admission.enter(id: "a", lane: .normal) { started.add("a") }
    admission.enter(id: "b", lane: .normal) { started.add("b") }
    admission.enter(id: "key", lane: .priority) { started.add("key") }
    #expect(started.values == ["a", "key"])
    admission.finish("key")  // it held nothing
    #expect(started.values == ["a", "key"])
    admission.finish("a")
    #expect(started.values == ["a", "key", "b"])
  }

  @Test func aRequestThatLeavesBeforeItStartedTakesNoSlotAndLeavesTheLine() {
    let admission = RequestAdmission(1)
    let started = Started()
    for id in ["a", "b", "c"] { admission.enter(id: id, lane: .normal) { started.add(id) } }
    admission.finish("b")  // timed out while it waited
    #expect(admission.waiting == 1)
    admission.finish("a")
    #expect(started.values == ["a", "c"])
  }
}
