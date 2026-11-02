import CoreGraphics

/// One key of the row above the phone's software keyboard. The table is data so the rules that
/// matter (the order, what a key sends, the size a finger gets) are tested without a screen.
struct PhoneTerminalKey: Equatable {
  enum Action: Equatable {
    case bytes([UInt8])
    case text(String)
    case arrow(Direction)
    case function(Int)
    case control
    case mouseReporting
    case dismissKeyboard
  }

  enum Direction: Equatable { case up, down, left, right }

  enum Face: Equatable {
    case title(String)
    case symbol(String)
  }

  let id: String
  let face: Face
  let label: String
  let action: Action

  /// Only the arrows repeat while held.
  var repeats: Bool {
    if case .arrow = action { return true }
    return false
  }

  /// A key is as wide and as tall as a finger needs, whatever it looks like: the drawn key is
  /// smaller (`PhoneTerminalKeyMetrics.keyInset`), the press area is not.
  var pressSize: CGSize { CGSize(width: Self.pressSide, height: Self.pressSide) }

  static let pressSide = PaneInteriorMetrics.touchTarget

  private static func title(_ id: String, _ title: String, _ label: String, _ action: Action)
    -> PhoneTerminalKey
  {
    PhoneTerminalKey(id: id, face: .title(title), label: label, action: action)
  }

  private static func symbol(_ id: String, _ name: String, _ label: String, _ action: Action)
    -> PhoneTerminalKey
  {
    PhoneTerminalKey(id: id, face: .symbol(name), label: label, action: action)
  }

  /// The keys that scroll sideways: the ones a shell and a full-screen program need first,
  /// then the symbols a phone keyboard buries, the touch switch and the function keys.
  static let scrolling: [PhoneTerminalKey] =
    [
      title("escape", "esc", "Escape", .bytes([0x1b])),
      title("control", "ctrl", "Control", .control),
      symbol("tab", "arrow.right.to.line.compact", "Tab", .bytes([0x09])),
      symbol("left", "arrow.left", "Left arrow", .arrow(.left)),
      symbol("down", "arrow.down", "Down arrow", .arrow(.down)),
      symbol("up", "arrow.up", "Up arrow", .arrow(.up)),
      symbol("right", "arrow.right", "Right arrow", .arrow(.right)),
      title("tilde", "~", "Tilde", .text("~")),
      title("pipe", "|", "Vertical bar", .text("|")),
      title("slash", "/", "Slash", .text("/")),
      title("dash", "-", "Hyphen", .text("-")),
      symbol("touch", "hand.draw", "Touch selects text", .mouseReporting),
    ]
    + (1...10).map { title("f\($0)", "F\($0)", "F\($0)", .function($0)) }

  /// Pinned at the trailing edge, so it never scrolls out of reach.
  static let dismiss = symbol(
    "dismiss", "keyboard.chevron.compact.down", "Hide keyboard", .dismissKeyboard)

  static var all: [PhoneTerminalKey] { scrolling + [dismiss] }
}
