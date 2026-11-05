import Foundation
import SoyehtProtocol
import SwiftTerm
import Testing

@testable import SoyehtClient

/// The terminal of the Mac, iPhone and iPad keeps the scrollback of the shared client policy,
/// which is the number the desktop client keeps. SwiftTerm's own default is 500 lines.
private final class Mute: TerminalDelegate {
  func send(source: Terminal, data: ArraySlice<UInt8>) {}
}

/// Feeds `count` numbered lines and returns the number of the first line the terminal still has
/// (a line that scrolled out of the scrollback is gone).
private func firstLineKept(_ terminal: Terminal, count: Int) throws -> Int {
  terminal.feed(text: (0..<count).map { "row \($0)\r\n" }.joined())
  let text = String(decoding: terminal.getBufferAsData(kind: .normal), as: UTF8.self)
  let first = try #require(text.split(separator: "\n").first)
  return try #require(Int(first.split(separator: " ").last ?? ""))
}

/// Feeds more lines than the policy keeps and expects the terminal to have dropped exactly the
/// ones beyond the scrollback and the screen (the screen's last row is the empty one after the
/// final line break).
private func expectPolicyScrollback(_ terminal: Terminal) throws {
  let capacity = ClientPolicy.terminalScrollbackLines + terminal.rows
  let total = capacity + 500
  #expect(try firstLineKept(terminal, count: total) == total - capacity + 1)
}

@Test func theStartupOptionsKeepThePolicyScrollbackAndEveryOtherDefault() {
  var options = TerminalOptions.soyehtStartup
  #expect(ClientPolicy.terminalScrollbackLines > TerminalOptions.default.scrollback)
  #expect(options.scrollback == ClientPolicy.terminalScrollbackLines)
  // TerminalOptions is not Equatable: with the scrollback put back, the rest reads the same.
  options.scrollback = TerminalOptions.default.scrollback
  #expect(String(describing: options) == String(describing: TerminalOptions.default))
}

@Test func aTerminalWithTheStartupOptionsKeepsTheScrollbackOfThePolicy() throws {
  let terminal = Terminal(delegate: Mute(), options: .soyehtStartup)
  try expectPolicyScrollback(terminal)
}

#if os(macOS)
  @MainActor @Test func theMacTerminalSurfaceStartsWithTheScrollbackOfThePolicy() throws {
    let surface = TerminalSurface(font: MacTerminalFont.active())
    let terminal = surface.getTerminal()
    #expect(terminal.options.scrollback == ClientPolicy.terminalScrollbackLines)
    try expectPolicyScrollback(terminal)
  }

  @MainActor @Test func aResetOfTheMacTerminalSurfaceKeepsTheScrollbackOfThePolicy() throws {
    let surface = TerminalSurface(font: MacTerminalFont.active())
    let bytes = Data("row\r\n".utf8)
    surface.receive(bytes, reset: false)
    surface.receive(bytes, reset: true)
    let terminal = surface.getTerminal()
    #expect(terminal.options.scrollback == ClientPolicy.terminalScrollbackLines)
    try expectPolicyScrollback(terminal)
  }
#endif

#if os(iOS)
  @MainActor @Test func thePhoneTerminalSurfaceStartsWithTheScrollbackOfThePolicy() {
    let surface = PhoneTerminalSurface(font: PhoneTerminal.font(TerminalFontSizePolicy.defaultSize))
    #expect(surface.getTerminal().options.scrollback == ClientPolicy.terminalScrollbackLines)
  }
#endif
