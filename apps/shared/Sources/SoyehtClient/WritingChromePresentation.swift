import Foundation
import SoyehtProtocol

/// What a writing chrome action says and how it is found by accessibility, so the menu and the
/// selection toolbar of every writing pane are drawn from `WritingChromePlan` alone.
extension WritingChromeAction {
  /// The label of a menu action. Focus mode names the move it makes from its current state.
  public func title(focusMode: Bool = false) -> String {
    switch self {
    case .font: "Font"
    case .useFullWidth: "Use full width"
    case .useReadingWidth: "Use reading width"
    case .focusMode: focusMode ? "Turn off Focus mode" : "Focus mode"
    case .ideaBoard: "Idea board"
    case .scenes: "Scenes"
    case .present: "Present"
    case .splitColumns: "Split into columns"
    case .splitRows: "Split into rows"
    case .closePane: "Close pane"
    case .bold: "Bold"
    case .italic: "Italic"
    case .heading: "Heading"
    case .list: "List"
    case .quote: "Quote"
    case .link: "Link"
    case .image: "Image"
    }
  }

  public var systemImage: String {
    switch self {
    case .font: "textformat"
    case .useFullWidth: "arrow.up.left.and.arrow.down.right"
    case .useReadingWidth: "arrow.down.right.and.arrow.up.left"
    case .focusMode: "scope"
    case .ideaBoard: "lightbulb"
    case .scenes: "sidebar.left"
    case .present: "play.fill"
    case .splitColumns: "rectangle.split.2x1"
    case .splitRows: "rectangle.split.1x2"
    case .closePane: "xmark"
    case .bold: "bold"
    case .italic: "italic"
    case .heading: "textformat.size"
    case .list: "list.bullet"
    case .quote: "text.quote"
    case .link: "link"
    case .image: "photo"
    }
  }

  public func accessibilityIdentifier(paneId: String) -> String {
    switch self {
    case .font: "writing-options-font-\(paneId)"
    case .useFullWidth, .useReadingWidth: "writing-options-width-\(paneId)"
    case .focusMode: "writing-options-focus-\(paneId)"
    case .ideaBoard: "writing-show-beat-board-\(paneId)"
    case .scenes: "writing-show-scenes-\(paneId)"
    case .present: "writing-present-\(paneId)"
    case .splitColumns: "split-pane-columns-\(paneId)"
    case .splitRows: "split-pane-rows-\(paneId)"
    case .closePane: "close-pane-\(paneId)"
    case .bold: "writing-selection-bold-\(paneId)"
    case .italic: "writing-selection-italic-\(paneId)"
    case .heading: "writing-selection-heading-\(paneId)"
    case .list: "writing-selection-list-\(paneId)"
    case .quote: "writing-selection-quote-\(paneId)"
    case .link: "writing-selection-link-\(paneId)"
    case .image: "writing-selection-image-\(paneId)"
    }
  }

  /// The key (with Command) that applies a selection action, if it has one.
  public var shortcutKey: Character? {
    switch self {
    case .bold: "b"
    case .italic: "i"
    case .link: "k"
    default: nil
    }
  }

  /// A selection action that opens a menu of formats instead of applying one.
  public var formatChoices: [(title: String, kind: WritingFormatKind)] {
    switch self {
    case .heading: [("Heading 1", .heading1), ("Heading 2", .heading2), ("Heading 3", .heading3)]
    case .list: [("Bulleted list", .bullet_list), ("Numbered list", .numbered_list)]
    default: []
    }
  }

  /// The format a selection action applies at once.
  public var directFormat: WritingFormatKind? {
    switch self {
    case .bold: .bold
    case .italic: .italic
    case .quote: .quote
    default: nil
    }
  }

  /// A selection action that asks for a path or an address before it applies.
  public var destinationFormat: WritingFormatKind? {
    switch self {
    case .link: .link
    case .image: .image
    default: nil
    }
  }
}

/// The text of the popover that asks for a link address or an image path.
extension WritingFormatKind {
  public var destinationPrompt: (title: String, placeholder: String)? {
    switch self {
    case .image: ("Image path", "images/cover.png")
    case .link: ("Link destination", "https://")
    default: nil
    }
  }
}

extension WritingChromeMetrics {
  /// How long the chrome takes to fade into `visibility`.
  public func fadeDuration(into visibility: WritingChromeVisibility) -> Double {
    visibility == .dimmed ? dimDuration : revealDuration
  }

  /// The opacity of the chrome in `visibility`.
  public func opacity(for visibility: WritingChromeVisibility) -> Double {
    visibility == .dimmed ? dimmedOpacity : 1
  }
}

extension WritingLayoutMetrics {
  /// The divider drawn along the edge of a navigation rail, to its right.
  public func railDivider(after navigationRect: PaneLayoutRect) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(x: navigationRect.maxX, y: navigationRect.origin.y),
      size: PaneLayoutSize(width: railDividerWidth, height: navigationRect.size.height))
  }
}
