public enum MacSheetKind: String, CaseIterable, Equatable, Sendable {
  case pairing
  case writingCreate
  case ideaEditor
  case editorHistory
  case editorConflict
  case editorReferences
  case editorDiff
  case reviewComment
  case captureDeletion
  case harnessEditor
  case preferences
}

public enum MacSheetNavigation: Equatable, Sendable {
  case sidebar
  case stack
}

public struct MacSheetLayoutPlan: Equatable, Sendable {
  public var width: Double
  public var height: Double?
  public var navigation: MacSheetNavigation

  public init(width: Double, height: Double?, navigation: MacSheetNavigation) {
    self.width = width
    self.height = height
    self.navigation = navigation
  }
}

public struct MacSheetLayoutMetrics: Equatable, Sendable {
  public var windowInset: Double
  public var compactPreferencesWidth: Double
  public var compactNavigationRowHeight: Double
  public var compactNavigationIconWidth: Double

  public init(
    windowInset: Double, compactPreferencesWidth: Double,
    compactNavigationRowHeight: Double, compactNavigationIconWidth: Double
  ) {
    self.windowInset = windowInset
    self.compactPreferencesWidth = compactPreferencesWidth
    self.compactNavigationRowHeight = compactNavigationRowHeight
    self.compactNavigationIconWidth = compactNavigationIconWidth
  }

  public static let standard = MacSheetLayoutMetrics(
    windowInset: 12,
    compactPreferencesWidth: 700,
    compactNavigationRowHeight: 40,
    compactNavigationIconWidth: 18)
}

/// One inventory and one fit rule for every fixed-size Mac sheet-like surface.
public struct MacSheetLayoutPolicy: Equatable, Sendable {
  public var metrics: MacSheetLayoutMetrics

  public init(metrics: MacSheetLayoutMetrics = .standard) {
    self.metrics = metrics
  }

  public func plan(
    for kind: MacSheetKind, available: PaneLayoutSize?
  ) -> MacSheetLayoutPlan {
    let preferred = preferredSize(for: kind)
    guard let available else {
      return MacSheetLayoutPlan(
        width: preferred.width, height: preferred.height, navigation: .sidebar)
    }
    let widthLimit = usable(available.width, fallback: preferred.width)
    let heightLimit = usable(available.height, fallback: preferred.height ?? 0)
    let width = min(preferred.width, widthLimit)
    let height = preferred.height.map { min($0, heightLimit) }
    let navigation: MacSheetNavigation =
      kind == .preferences && width < metrics.compactPreferencesWidth ? .stack : .sidebar
    return MacSheetLayoutPlan(width: width, height: height, navigation: navigation)
  }

  public func preferredSize(for kind: MacSheetKind) -> (width: Double, height: Double?) {
    switch kind {
    case .pairing: (520, 720)
    case .writingCreate: (430, nil)
    case .ideaEditor: (520, nil)
    case .editorHistory: (820, 560)
    case .editorConflict: (920, 580)
    case .editorReferences: (560, 420)
    case .editorDiff: (980, 620)
    case .reviewComment: (480, nil)
    case .captureDeletion: (420, nil)
    case .harnessEditor: (560, nil)
    case .preferences: (1080, 700)
    }
  }

  private func usable(_ available: Double, fallback: Double) -> Double {
    guard available.isFinite, available > 0 else { return fallback }
    return max(0, available - 2 * metrics.windowInset)
  }
}

public struct WritingBeatBoardHeaderPlan: Equatable, Sendable {
  public var showsActionTitles: Bool

  public init(showsActionTitles: Bool) {
    self.showsActionTitles = showsActionTitles
  }
}

public struct WritingBeatBoardHeaderPolicy: Equatable, Sendable {
  public var titledActionsMinimumWidth: Double

  public init(titledActionsMinimumWidth: Double = 520) {
    self.titledActionsMinimumWidth = titledActionsMinimumWidth
  }

  public func plan(viewportWidth: Double) -> WritingBeatBoardHeaderPlan {
    WritingBeatBoardHeaderPlan(
      showsActionTitles: viewportWidth.isFinite && viewportWidth >= titledActionsMinimumWidth)
  }
}
