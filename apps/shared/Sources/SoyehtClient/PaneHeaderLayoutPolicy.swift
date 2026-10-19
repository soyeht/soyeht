import Foundation

public enum PaneHeaderActionPlacement: Equatable, Sendable {
  case inline
  case menu
}

public struct PaneHeaderLayoutPlan: Equatable, Sendable {
  public var actionPlacement: PaneHeaderActionPlacement
  /// The room the activity line gets, or nil when the header drops it.
  public var summaryMaximumWidth: Double?
  /// False only when the header cannot hold the title and the status word whole.
  public var showsStatus: Bool

  public init(
    actionPlacement: PaneHeaderActionPlacement,
    summaryMaximumWidth: Double?,
    showsStatus: Bool = true
  ) {
    self.actionPlacement = actionPlacement
    self.summaryMaximumWidth = summaryMaximumWidth
    self.showsStatus = showsStatus
  }
}

public struct PaneHeaderLayoutMetrics: Equatable, Sendable {
  public var horizontalPadding: Double
  public var spacing: Double
  public var stateDotWidth: Double
  public var readableTitleWidth: Double
  public var summaryWidth: Double
  public var statusWidth: Double
  public var actionWidth: Double

  public init(
    horizontalPadding: Double, spacing: Double, stateDotWidth: Double,
    readableTitleWidth: Double, summaryWidth: Double, statusWidth: Double,
    actionWidth: Double
  ) {
    self.horizontalPadding = horizontalPadding
    self.spacing = spacing
    self.stateDotWidth = stateDotWidth
    self.readableTitleWidth = readableTitleWidth
    self.summaryWidth = summaryWidth
    self.statusWidth = statusWidth
    self.actionWidth = actionWidth
  }
}

/// What a pane header keeps when its width runs out, in the order it gives things up:
/// 1. the actions fold into one menu;
/// 2. the activity line is dropped, once it would get less than its minimum;
/// 3. the status word is dropped.
/// The dot, the title (truncated with an ellipsis, never below its readable width) and the
/// menu always stay, and the status word is never truncated: it is the shortest text and the
/// one that asks for the person.
public struct PaneHeaderLayoutPolicy: Equatable, Sendable {
  public var metrics: PaneHeaderLayoutMetrics

  public init(metrics: PaneHeaderLayoutMetrics) {
    self.metrics = metrics
  }

  public func plan(
    availableWidth: Double, actionCount: Int, hasSummary: Bool, hasStatus: Bool
  ) -> PaneHeaderLayoutPlan {
    let inlineWidth = requiredWidth(
      actionCount: actionCount, includesSummary: hasSummary, includesStatus: hasStatus)
    if availableWidth >= inlineWidth {
      return PaneHeaderLayoutPlan(actionPlacement: .inline, summaryMaximumWidth: nil)
    }

    let compactWithSummary = requiredWidth(
      actionCount: 1, includesSummary: hasSummary, includesStatus: hasStatus)
    let compact = requiredWidth(actionCount: 1, includesSummary: false, includesStatus: hasStatus)
    return PaneHeaderLayoutPlan(
      actionPlacement: .menu,
      summaryMaximumWidth: hasSummary && availableWidth >= compactWithSummary
        ? metrics.summaryWidth : nil,
      showsStatus: !hasStatus || availableWidth >= compact)
  }

  public func requiredWidth(
    actionCount: Int, includesSummary: Bool, includesStatus: Bool
  ) -> Double {
    let fixedItemWidths =
      metrics.stateDotWidth + metrics.readableTitleWidth
      + (includesStatus ? metrics.statusWidth : 0)
      + Double(actionCount) * metrics.actionWidth
      + (includesSummary ? metrics.summaryWidth : 0)
    let itemCount = 2 + actionCount + (includesSummary ? 1 : 0) + (includesStatus ? 1 : 0)
    return metrics.horizontalPadding + fixedItemWidths
      + Double(max(0, itemCount - 1)) * metrics.spacing
  }
}
