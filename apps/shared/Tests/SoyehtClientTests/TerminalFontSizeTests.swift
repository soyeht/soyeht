import Foundation
import Testing

@testable import SoyehtClient

@Test func terminalFontSizeLimitsAreTheDesignTokens() {
  #expect(TerminalFontSizePolicy.defaultSize == Double(DesignTokens.PaneLayout.terminalFontSize))
  #expect(TerminalFontSizePolicy.minimum == Double(DesignTokens.PaneLayout.terminalFontSizeMinimum))
  #expect(TerminalFontSizePolicy.maximum == Double(DesignTokens.PaneLayout.terminalFontSizeMaximum))
  #expect(TerminalFontSizePolicy.minimum < TerminalFontSizePolicy.defaultSize)
  #expect(TerminalFontSizePolicy.defaultSize < TerminalFontSizePolicy.maximum)
}

@Test func terminalFontSizeStaysInsideTheLimitsAndOnWholeSteps() {
  let low = TerminalFontSizePolicy.minimum
  let high = TerminalFontSizePolicy.maximum
  #expect(TerminalFontSizePolicy.clamped(1) == low)
  #expect(TerminalFontSizePolicy.clamped(500) == high)
  #expect(TerminalFontSizePolicy.clamped(13.4) == 13)
  #expect(TerminalFontSizePolicy.clamped(13.6) == 14)
  #expect(TerminalFontSizePolicy.clamped(.nan) == TerminalFontSizePolicy.defaultSize)
  #expect(TerminalFontSizePolicy.clamped(.infinity) == TerminalFontSizePolicy.defaultSize)
  #expect(TerminalFontSizePolicy.stepped(high, by: 1) == high)
  #expect(TerminalFontSizePolicy.stepped(low, by: -1) == low)
  #expect(TerminalFontSizePolicy.stepped(13, by: 2) == 15)
}

@Test func pinchingScalesTheSizeFromWhereItBegan() {
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 1) == 13)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 2) == 26)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 0.7) == 9)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 10) == TerminalFontSizePolicy.maximum)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 0.01) == TerminalFontSizePolicy.minimum)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: 0) == 13)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: -2) == 13)
  #expect(TerminalFontSizePolicy.pinched(from: 13, scale: .nan) == 13)
}

@Test func commandKeysMapToZoomCommands() {
  #expect(TerminalZoomShortcut.matching(input: "=", shift: false)?.command == .larger)
  #expect(TerminalZoomShortcut.matching(input: "+", shift: true)?.command == .larger)
  #expect(TerminalZoomShortcut.matching(input: "-", shift: false)?.command == .smaller)
  #expect(TerminalZoomShortcut.matching(input: "0", shift: false)?.command == .reset)
  #expect(TerminalZoomShortcut.matching(input: "=", shift: true) == nil)
  #expect(TerminalZoomShortcut.matching(input: "a", shift: false) == nil)
  let inputs = TerminalZoomShortcut.all.map { "\($0.input)\($0.shift)" }
  #expect(Set(inputs).count == inputs.count)
}

@MainActor @Test func terminalFontSizeStoreStepsAndRemembersTheSize() {
  let preferences = MemoryPreferenceStore()
  let store = TerminalFontSizeStore(preferences: preferences)
  #expect(store.size == TerminalFontSizePolicy.defaultSize)

  store.apply(.larger)
  store.apply(.larger)
  #expect(store.size == TerminalFontSizePolicy.defaultSize + 2)
  store.apply(.smaller)
  #expect(store.size == TerminalFontSizePolicy.defaultSize + 1)
  #expect(TerminalFontSizeStore(preferences: preferences).size == store.size)

  store.apply(.reset)
  #expect(store.size == TerminalFontSizePolicy.defaultSize)
}

@MainActor @Test func aPinchSavesTheSizeOnceWhenItEnds() {
  let preferences = MemoryPreferenceStore()
  let store = TerminalFontSizeStore(preferences: preferences)
  store.set(15, persisting: false)
  store.set(17, persisting: false)
  #expect(store.size == 17)
  #expect(preferences.integers[TerminalFontSizeStore.sizeKey] == nil)

  store.commit()
  #expect(preferences.integers[TerminalFontSizeStore.sizeKey] == 17)
  #expect(TerminalFontSizeStore(preferences: preferences).size == 17)
}

@MainActor @Test func terminalFontSizeStoreStopsAtTheLimits() {
  let store = TerminalFontSizeStore(preferences: MemoryPreferenceStore())
  store.set(1000)
  #expect(store.size == TerminalFontSizePolicy.maximum)
  #expect(!store.canGrow)
  #expect(store.canShrink)
  store.set(-4)
  #expect(store.size == TerminalFontSizePolicy.minimum)
  #expect(store.canGrow)
  #expect(!store.canShrink)
}

@MainActor @Test func terminalFontSizeStoreClampsASavedSizeOutsideTheLimits() {
  let preferences = MemoryPreferenceStore()
  preferences.integers[TerminalFontSizeStore.sizeKey] = 4000
  #expect(TerminalFontSizeStore(preferences: preferences).size == TerminalFontSizePolicy.maximum)
}
