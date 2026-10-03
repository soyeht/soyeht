import Foundation

public enum PaneHeaderActionPlacement: Equatable, Sendable {
  case inline
  case menu
}

public struct PaneHeaderLayoutPlan: Equatable, Sendable {
  public var actionPlacement: PaneHeaderActionPlacement
  public var summaryMaximumWidth: Double?

  public init(
    actionPlacement: PaneHeaderActionPlacement,
    summaryMaximumWidth: Double?
  ) {
    self.actionPlacement = actionPlacement
    self.summaryMaximumWidth = summaryMaximumWidth
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
  /// Width the harness menu button of an agent pane ("Claude ▾") may take. It stays visible
  /// when the other actions collapse into the menu.
  public var harnessControlWidth: Double

  public init(
    horizontalPadding: Double, spacing: Double, stateDotWidth: Double,
    readableTitleWidth: Double, summaryWidth: Double, statusWidth: Double,
    actionWidth: Double, harnessControlWidth: Double = 0
  ) {
    self.horizontalPadding = horizontalPadding
    self.spacing = spacing
    self.stateDotWidth = stateDotWidth
    self.readableTitleWidth = readableTitleWidth
    self.summaryWidth = summaryWidth
    self.statusWidth = statusWidth
    self.actionWidth = actionWidth
    self.harnessControlWidth = harnessControlWidth
  }
}

public struct PaneHeaderLayoutPolicy: Equatable, Sendable {
  public var metrics: PaneHeaderLayoutMetrics

  public init(metrics: PaneHeaderLayoutMetrics) {
    self.metrics = metrics
  }

  public func plan(
    availableWidth: Double, actionCount: Int, hasSummary: Bool, hasStatus: Bool,
    hasHarnessControl: Bool = false
  ) -> PaneHeaderLayoutPlan {
    let inlineWidth = requiredWidth(
      actionCount: actionCount, includesSummary: hasSummary, includesStatus: hasStatus,
      includesHarnessControl: hasHarnessControl)
    if availableWidth >= inlineWidth {
      return PaneHeaderLayoutPlan(actionPlacement: .inline, summaryMaximumWidth: nil)
    }

    let compactWithSummary = requiredWidth(
      actionCount: 1, includesSummary: hasSummary, includesStatus: hasStatus,
      includesHarnessControl: hasHarnessControl)
    return PaneHeaderLayoutPlan(
      actionPlacement: .menu,
      summaryMaximumWidth: hasSummary && availableWidth >= compactWithSummary
        ? metrics.summaryWidth : nil)
  }

  public func requiredWidth(
    actionCount: Int, includesSummary: Bool, includesStatus: Bool,
    includesHarnessControl: Bool = false
  ) -> Double {
    let fixedItemWidths =
      metrics.stateDotWidth + metrics.readableTitleWidth
      + (includesStatus ? metrics.statusWidth : 0)
      + Double(actionCount) * metrics.actionWidth
      + (includesSummary ? metrics.summaryWidth : 0)
      + (includesHarnessControl ? metrics.harnessControlWidth : 0)
    let itemCount =
      2 + actionCount + (includesSummary ? 1 : 0) + (includesStatus ? 1 : 0)
      + (includesHarnessControl ? 1 : 0)
    return metrics.horizontalPadding + fixedItemWidths
      + Double(max(0, itemCount - 1)) * metrics.spacing
  }
}
