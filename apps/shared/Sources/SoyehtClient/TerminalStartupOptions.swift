import SoyehtProtocol
import SwiftTerm

extension TerminalOptions {
  /// What every terminal surface of the Mac, iPhone and iPad starts with: SwiftTerm's defaults,
  /// except that the scrollback is the shared policy's (`ClientPolicy.terminalScrollbackLines`,
  /// the same number the desktop client keeps). SwiftTerm's own default is 500 lines. A reset
  /// (`resetToInitialState`) keeps the options of the terminal it resets, so this is the one
  /// place the number enters.
  static var soyehtStartup: TerminalOptions {
    var options = TerminalOptions.default
    options.scrollback = ClientPolicy.terminalScrollbackLines
    return options
  }
}
