import Foundation
import SoyehtProtocol

#if canImport(UIKit)
  import UIKit
#elseif canImport(AppKit)
  import AppKit
#endif

/// How a screenplay line sits on the page, whatever the platform draws it with: alignment,
/// indents, weight, the markers that stay hidden. The Mac and the iPhone apply the same value
/// to their own paragraph styles and fonts.
public struct ScreenplayLineStyle: Equatable, Sendable {
  public enum Alignment: Sendable {
    case natural
    case center
    case left
    case right
  }

  /// The margin of a dialogue block on each side.
  public static let dialogueInset = 120.0
  /// The space after each paragraph of the page.
  public static let paragraphSpacing = 8.0
  /// The size of a marker character that is kept in the text but not shown.
  public static let hiddenMarkerFontSize = 0.1

  public var alignment: Alignment = .natural
  public var headIndent = 0.0
  public var tailIndent = 0.0
  public var bold = false
  /// Drawn in the secondary ink instead of the primary one.
  public var secondary = false
  public var hiddenRanges: [NSRange] = []

  public init(kind: ScreenplayElementKind, line: String) {
    let projection = ScreenplayInteraction.lineProjection(source: line, kind: kind)
    hiddenRanges = projection.hiddenRanges
    switch kind {
    case .scene_heading, .shot:
      bold = true
    case .action:
      break
    case .character:
      alignment = .center
      bold = true
    case .parenthetical:
      alignment = .center
      secondary = true
    case .dialogue:
      headIndent = Self.dialogueInset
      tailIndent = -Self.dialogueInset
    case .transition:
      alignment = projection.openingTransition ? .left : .right
      secondary = true
    }
  }
}

extension ScreenplayLineStyle {
  /// The paragraph every line of the page starts from: the screenplay's line height and the
  /// space after a paragraph.
  public static func basePageParagraph() -> NSMutableParagraphStyle {
    let style = NSMutableParagraphStyle()
    style.minimumLineHeight = PaneLayoutContentMetrics.screenplayLineHeight
    style.maximumLineHeight = PaneLayoutContentMetrics.screenplayLineHeight
    style.paragraphSpacing = paragraphSpacing
    return style
  }

  /// The paragraph this line is set in.
  public func paragraphStyle() -> NSMutableParagraphStyle {
    let style = Self.basePageParagraph()
    switch alignment {
    case .natural: style.alignment = .natural
    case .center: style.alignment = .center
    case .left: style.alignment = .left
    case .right: style.alignment = .right
    }
    style.firstLineHeadIndent = headIndent
    style.headIndent = headIndent
    style.tailIndent = tailIndent
    return style
  }

  /// The markers of the line that starts at `lineRange`, as ranges of the whole text and kept
  /// inside the line.
  public func hiddenMarkerRanges(inLine lineRange: NSRange) -> [NSRange] {
    hiddenRanges.compactMap { marker in
      let absolute = NSRange(location: lineRange.location + marker.location, length: marker.length)
      return NSMaxRange(absolute) <= NSMaxRange(lineRange) ? absolute : nil
    }
  }
}

/// The fonts and inks a platform draws a screenplay page with. They are values of the
/// platform's own font and color types.
public struct ScreenplayPageAppearance {
  public var boldFont: Any
  public var secondaryInk: Any
  public var hiddenMarkerFont: Any
  public var hiddenMarkerInk: Any

  public init(boldFont: Any, secondaryInk: Any, hiddenMarkerFont: Any, hiddenMarkerInk: Any) {
    self.boldFont = boldFont
    self.secondaryInk = secondaryInk
    self.hiddenMarkerFont = hiddenMarkerFont
    self.hiddenMarkerInk = hiddenMarkerInk
  }
}

extension ScreenplayLineStyle {
  /// Sets the line at `range` of `text` the way this style says.
  public func apply(
    to text: NSMutableAttributedString, range: NSRange, appearance: ScreenplayPageAppearance
  ) {
    if bold { text.addAttribute(.font, value: appearance.boldFont, range: range) }
    if secondary {
      text.addAttribute(.foregroundColor, value: appearance.secondaryInk, range: range)
    }
    text.addAttribute(.paragraphStyle, value: paragraphStyle(), range: range)
    for marker in hiddenMarkerRanges(inLine: range) {
      text.addAttributes(
        [.font: appearance.hiddenMarkerFont, .foregroundColor: appearance.hiddenMarkerInk],
        range: marker)
    }
  }
}
