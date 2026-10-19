import Foundation
import Testing

@testable import SoyehtClient

@Test func copiedTextIsBoundedValidUTF8() {
  #expect(TerminalClipboardPolicy.copiedText(Data("hello".utf8)) == "hello")
  #expect(TerminalClipboardPolicy.copiedText(Data()) == nil)
  #expect(TerminalClipboardPolicy.copiedText(Data([0xff, 0xfe, 0xfd])) == nil)
  let limit = TerminalClipboardPolicy.maximumCopiedBytes
  #expect(TerminalClipboardPolicy.copiedText(Data(repeating: 0x61, count: limit))?.count == limit)
  #expect(TerminalClipboardPolicy.copiedText(Data(repeating: 0x61, count: limit + 1)) == nil)
}

@Test func scrollReachesTheProgramThatWantsIt() {
  #expect(TerminalWheelTarget(mouseReporting: false, alternateScreen: false) == .scrollback)
  #expect(TerminalWheelTarget(mouseReporting: true, alternateScreen: false) == .mouseWheel)
  #expect(TerminalWheelTarget(mouseReporting: true, alternateScreen: true) == .mouseWheel)
  #expect(TerminalWheelTarget(mouseReporting: false, alternateScreen: true) == .arrowKeys)
}

@Test func scrollDistanceBecomesWholeLinesAndKeepsTheRemainder() {
  var wheel = TerminalWheelAccumulator()
  #expect(wheel.lines(delta: 8, lineHeight: 20) == 0)
  #expect(wheel.lines(delta: 8, lineHeight: 20) == 0)
  #expect(wheel.lines(delta: 8, lineHeight: 20) == 1)  // 24 points: one line, 4 carried
  #expect(wheel.lines(delta: -44, lineHeight: 20) == -2)  // 4 - 44 = -40
  wheel.reset()
  #expect(wheel.lines(delta: 19, lineHeight: 20) == 0)
}

@Test func aFastFlickIsCappedAndBadInputIsIgnored() {
  var wheel = TerminalWheelAccumulator()
  #expect(
    wheel.lines(delta: 10_000, lineHeight: 20) == TerminalWheelAccumulator.maximumLinesPerEvent)
  #expect(
    wheel.lines(delta: -10_000, lineHeight: 20) == -TerminalWheelAccumulator.maximumLinesPerEvent)
  #expect(wheel.lines(delta: .nan, lineHeight: 20) == 0)
  #expect(wheel.lines(delta: 5, lineHeight: 0) == 0)
}
