import Foundation
import SoyehtProtocol

public enum WritingChromeAction: String, CaseIterable, Equatable, Sendable {
  case font
  case useFullWidth
  case useReadingWidth
  case focusMode
  case ideaBoard
  case scenes
  case present
  case splitColumns
  case splitRows
  case closePane
  case bold
  case italic
  case heading
  case list
  case quote
  case link
  case image
}

public struct WritingChromePlan: Equatable, Sendable {
  public let menuActions: [WritingChromeAction]
  public let selectionActions: [WritingChromeAction]

  public init(
    menuActions: [WritingChromeAction], selectionActions: [WritingChromeAction]
  ) {
    self.menuActions = menuActions
    self.selectionActions = selectionActions
  }
}

public struct WritingChromeMetrics: Equatable, Sendable {
  public let headerHeight: Double
  public let headerHorizontalInset: Double
  public let headerFontPointSize: Double
  public let headerActionGap: Double
  public let headerMenuIconSize: Double
  public let headerMenuTargetSize: Double
  public let toolbarSize: PaneLayoutSize
  public let toolbarButtonWidth: Double
  public let toolbarIconSize: Double
  public let toolbarGap: Double
  public let toolbarHorizontalInset: Double
  public let selectionGap: Double
  public let toolbarShadowOffset: Double
  public let toolbarShadowBlur: Double
  public let toolbarHalo: Double
  public let dimmedOpacity: Double
  public let dimDuration: Double
  public let revealDuration: Double

  public static let standard = WritingChromeMetrics(
    headerHeight: PaneLayoutContentMetrics.writingHeaderHeight,
    headerHorizontalInset: PaneLayoutContentMetrics.writingHeaderHorizontalInset,
    headerFontPointSize: PaneLayoutContentMetrics.writingHeaderFontPointSize,
    headerActionGap: PaneLayoutContentMetrics.writingHeaderActionGap,
    headerMenuIconSize: PaneLayoutContentMetrics.writingHeaderMenuIconSize,
    headerMenuTargetSize: PaneLayoutContentMetrics.writingHeaderMenuTargetSize,
    toolbarSize: PaneLayoutSize(
      width: PaneLayoutContentMetrics.writingSelectionToolbarWidth,
      height: PaneLayoutContentMetrics.writingSelectionToolbarHeight),
    toolbarButtonWidth: PaneLayoutContentMetrics.writingSelectionToolbarButtonWidth,
    toolbarIconSize: PaneLayoutContentMetrics.writingSelectionToolbarIconSize,
    toolbarGap: PaneLayoutContentMetrics.writingSelectionToolbarGap,
    toolbarHorizontalInset: PaneLayoutContentMetrics.writingSelectionToolbarHorizontalInset,
    selectionGap: PaneLayoutContentMetrics.writingSelectionToolbarSelectionGap,
    toolbarShadowOffset: PaneLayoutContentMetrics.writingSelectionToolbarShadowOffset,
    toolbarShadowBlur: PaneLayoutContentMetrics.writingSelectionToolbarShadowBlur,
    toolbarHalo: PaneLayoutContentMetrics.writingSelectionToolbarHalo,
    dimmedOpacity: PaneLayoutContentMetrics.writingChromeDimmedOpacity,
    dimDuration: PaneLayoutContentMetrics.writingChromeDimDuration,
    revealDuration: PaneLayoutContentMetrics.writingChromeRevealDuration)
}

public enum WritingChromeVisibility: Equatable, Sendable {
  case visible
  case dimmed
}

public enum WritingChromeActivityEvent: Equatable, Sendable {
  case edited
  case pointerMoved
  case selectionChanged(Bool)
  case menuChanged(Bool)
  case popoverChanged(Bool)
  case headerHoverChanged(Bool)
  case paneDragChanged(Bool)
}

public struct WritingChromeActivityState: Equatable, Sendable {
  private var dimmedByEditing = false
  private var hasSelection = false
  private var menuOpen = false
  private var popoverOpen = false
  private var headerHovered = false
  private var paneDragging = false

  public init() {}

  public var visibility: WritingChromeVisibility {
    if hasSelection || menuOpen || popoverOpen || headerHovered || paneDragging { return .visible }
    return dimmedByEditing ? .dimmed : .visible
  }

  public mutating func reduce(_ event: WritingChromeActivityEvent) {
    switch event {
    case .edited:
      dimmedByEditing = true
    case .pointerMoved:
      dimmedByEditing = false
    case .selectionChanged(let value):
      hasSelection = value
    case .menuChanged(let value):
      menuOpen = value
    case .popoverChanged(let value):
      popoverOpen = value
    case .headerHoverChanged(let value):
      headerHovered = value
    case .paneDragChanged(let value):
      paneDragging = value
    }
  }
}

public enum WritingCloseDisposition: Equatable, Sendable {
  case closeImmediately
  case confirm(title: String, message: String)
}

public struct WritingChromePolicy: Equatable, Sendable {
  public let metrics: WritingChromeMetrics

  public init(metrics: WritingChromeMetrics = .standard) { self.metrics = metrics }

  public static func formatShortcut(key: String, command: Bool) -> WritingFormatKind? {
    guard command else { return nil }
    switch key.lowercased() {
    case "b": return .bold
    case "i": return .italic
    default: return nil
    }
  }

  public func plan(
    kind: PaneLayoutKind, fullWidth: Bool = false, hasSelection: Bool = false
  ) -> WritingChromePlan {
    switch kind {
    case .text:
      return WritingChromePlan(
        menuActions: [
          .font, fullWidth ? .useReadingWidth : .useFullWidth, .focusMode,
          .splitColumns, .splitRows, .closePane,
        ],
        selectionActions: hasSelection
          ? [.bold, .italic, .heading, .list, .quote, .link, .image] : [])
    case .screenplay:
      return WritingChromePlan(
        menuActions: [
          .ideaBoard, .scenes, .focusMode, .splitColumns, .splitRows, .closePane,
        ], selectionActions: [])
    case .presentation:
      return WritingChromePlan(
        menuActions: [.present, .splitColumns, .splitRows, .closePane],
        selectionActions: [])
    default:
      return WritingChromePlan(menuActions: [], selectionActions: [])
    }
  }

  public func selectionToolbarRect(
    selection: PaneLayoutRect, within available: PaneLayoutRect
  ) -> PaneLayoutRect {
    let safe = inset(available, by: metrics.toolbarHalo)
    let width = min(metrics.toolbarSize.width, safe.size.width)
    let height = metrics.toolbarSize.height
    let centeredX = selection.origin.x + (selection.size.width - width) / 2
    let x = clamp(centeredX, minimum: safe.origin.x, maximum: safe.maxX - width)
    let above = selection.origin.y - metrics.selectionGap - height
    let below = selection.maxY + metrics.selectionGap
    let preferredY = above >= safe.origin.y ? above : below
    let y = clamp(preferredY, minimum: safe.origin.y, maximum: safe.maxY - height)
    return PaneLayoutRect(
      origin: PaneLayoutPoint(x: x, y: y), size: PaneLayoutSize(width: width, height: height))
  }

  public func closeDisposition(fileName: String, dirty: Bool) -> WritingCloseDisposition {
    guard dirty else { return .closeImmediately }
    return .confirm(
      title: "Close \(fileName)?",
      message: "Unsaved changes will be lost.")
  }

  public func showsScreenplayElementMenu(
    lineText: String, selectionLength: Int
  ) -> Bool {
    selectionLength == 0
      && lineText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private func inset(_ rect: PaneLayoutRect, by amount: Double) -> PaneLayoutRect {
    let width = max(0, rect.size.width - 2 * amount)
    let height = max(0, rect.size.height - 2 * amount)
    return PaneLayoutRect(
      origin: PaneLayoutPoint(x: rect.origin.x + amount, y: rect.origin.y + amount),
      size: PaneLayoutSize(width: width, height: height))
  }

  private func clamp(_ value: Double, minimum: Double, maximum: Double) -> Double {
    guard maximum >= minimum else { return minimum }
    return min(max(value, minimum), maximum)
  }
}
