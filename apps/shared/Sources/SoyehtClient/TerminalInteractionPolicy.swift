import Foundation

/// What a program in the terminal may put on the system clipboard (an OSC 52 write). The
/// person pastes with their own gesture; a program never reads the clipboard.
public enum TerminalClipboardPolicy {
  public static let maximumCopiedBytes = 1 << 20

  /// A write is taken only when the person allowed it, the app is in front and the terminal
  /// is on screen: a program in a pane nobody is looking at does not touch the clipboard.
  public static func accepts(allowed: Bool, appActive: Bool, visible: Bool) -> Bool {
    allowed && appActive && visible
  }

  public static func copiedText(_ content: Data) -> String? {
    guard !content.isEmpty, content.count <= maximumCopiedBytes else { return nil }
    return String(data: content, encoding: .utf8)
  }
}

/// How a scroll from a trackpad or mouse wheel reaches a terminal. The scrollback is the
/// view's own scrolling. A program that asked for mouse reports gets wheel events, and a
/// full-screen program with no scrollback of its own (an editor, a pager) gets arrow keys, as
/// other terminals do.
public enum TerminalWheelTarget: Equatable, Sendable {
  case scrollback
  case mouseWheel
  case arrowKeys

  public init(mouseReporting: Bool, alternateScreen: Bool) {
    if mouseReporting {
      self = .mouseWheel
    } else if alternateScreen {
      self = .arrowKeys
    } else {
      self = .scrollback
    }
  }
}

/// Turns the distance a scroll moved into whole lines, keeping the remainder for the next event.
public struct TerminalWheelAccumulator: Equatable, Sendable {
  public static let maximumLinesPerEvent = 8

  private var carried = 0.0

  public init() {}

  /// Positive `delta` is content moving down, which reveals earlier lines: the wheel goes up.
  /// The result is positive for up and negative for down, in whole lines.
  public mutating func lines(delta: Double, lineHeight: Double) -> Int {
    guard delta.isFinite, lineHeight > 0 else { return 0 }
    carried += delta / lineHeight
    let whole = Int(carried.rounded(.towardZero))
    carried -= Double(whole)
    return max(-Self.maximumLinesPerEvent, min(Self.maximumLinesPerEvent, whole))
  }

  public mutating func reset() { carried = 0 }
}
