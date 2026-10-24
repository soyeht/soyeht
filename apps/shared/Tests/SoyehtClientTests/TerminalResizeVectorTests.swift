import Foundation
import SoyehtProtocol
import SwiftTerm
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/terminal-resize.json`, which the desktop client passes as
/// well: what a screen shows after its size changed. The Mac's terminal is SwiftTerm's.
private final class Mute: TerminalDelegate {
  func send(source: Terminal, data: ArraySlice<UInt8>) {}
}

@Test func aTerminalReflowsLikeTheDesktopClientDoes() throws {
  let vector = try ContractVector.load("terminal-resize")
  let delegate = Mute()
  for item in ContractVector.cases(vector, "reflow") {
    let name = item["name"].string ?? ""
    let terminal = Terminal(
      delegate: delegate,
      options: TerminalOptions(
        cols: try #require(item["cols"].int), rows: try #require(item["rows"].int),
        scrollback: ClientPolicy.terminalScrollbackLines))
    terminal.feed(text: try #require(item["output"].string))
    for size in item["resizes"].array ?? [] {
      let pair = size.array ?? []
      terminal.resize(cols: try #require(pair[1].int), rows: try #require(pair[0].int))
    }
    let expected = (item["screen"].array ?? []).map { $0.string ?? "" }
    let rows = (0..<expected.count).map { row in
      let text =
        terminal.getLine(row: row)?.translateToString(
          trimRight: true, skipNullCellsFollowingWide: true) ?? "<none>"
      return text.replacingOccurrences(of: " +$", with: "", options: .regularExpression)
    }
    #expect(rows == expected, "\(name)")
    if let expectedCursor = item["cursor"].array?.map({ $0.int }) {
      let cursor = terminal.getCursorLocation()
      #expect([cursor.y, cursor.x] == expectedCursor, "\(name)")
    }
    for style in item["styles"].array ?? [] {
      let at = (row: try #require(style["row"].int), column: try #require(style["column"].int))
      let attribute = try #require(terminal.getCharData(col: at.column, row: at.row)).attribute
      let fg: Int? = {
        if case .ansi256(let code) = attribute.fg { return Int(code) }
        return nil
      }()
      #expect(fg == style["fg"].int, "\(name) \(at)")
      #expect(attribute.style.contains(.bold) == (style["bold"].bool ?? false), "\(name) \(at)")
    }
  }
}
