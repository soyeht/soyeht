import Testing

@testable import SoyehtClient

@Test func everyPhoneTerminalKeyHasAFingerSizedPressArea() {
  for key in PhoneTerminalKey.all {
    #expect(key.pressSize.width >= 44, "\(key.id)")
    #expect(key.pressSize.height >= 44, "\(key.id)")
  }
  #expect(PaneInteriorMetrics.touchTarget == 44)
}

@Test func theFirstPhoneTerminalKeysAreEscapeControlTabAndTheArrows() {
  let ids = PhoneTerminalKey.scrolling.prefix(7).map(\.id)
  #expect(ids == ["escape", "control", "tab", "left", "down", "up", "right"])
  #expect(PhoneTerminalKey.scrolling[0].action == .bytes([0x1b]))
  #expect(PhoneTerminalKey.scrolling[2].action == .bytes([0x09]))
  #expect(PhoneTerminalKey.dismiss.action == .dismissKeyboard)
}

@Test func phoneTerminalKeysAreUniqueAndOnlyTheArrowsRepeat() {
  let ids = PhoneTerminalKey.all.map(\.id)
  #expect(Set(ids).count == ids.count)
  let repeating = PhoneTerminalKey.all.filter(\.repeats).map(\.id)
  #expect(repeating == ["left", "down", "up", "right"])
  #expect(PhoneTerminalKey.all.allSatisfy { !$0.label.isEmpty })
}
