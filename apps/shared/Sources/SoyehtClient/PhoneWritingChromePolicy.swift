import Foundation

public enum PhoneWritingChromeTransition: Equatable, Sendable {
  case dim
  case reveal
}

public struct PhoneWritingChromePlan: Equatable, Sendable {
  public var menuGroups: [[WritingChromeAction]]
  public var selectionActions: [WritingChromeAction]
  public var splitKinds: [PaneLayoutKind]
  public var headerHeight: Double
  public var minimumActionTarget: Double

  public var menuActions: [WritingChromeAction] { menuGroups.flatMap { $0 } }

  public init(
    menuGroups: [[WritingChromeAction]], selectionActions: [WritingChromeAction],
    splitKinds: [PaneLayoutKind], headerHeight: Double,
    minimumActionTarget: Double
  ) {
    self.menuGroups = menuGroups
    self.selectionActions = selectionActions
    self.splitKinds = splitKinds
    self.headerHeight = headerHeight
    self.minimumActionTarget = minimumActionTarget
  }
}

public enum PhoneWritingSheetKind: CaseIterable, Equatable, Sendable {
  case scenes
  case ideaBoard
  case split
  case link
  case image
}

public enum PhoneWritingSheetDetent: Equatable, Sendable {
  case medium
  case large
}

public struct PhoneWritingSheetPlan: Equatable, Sendable {
  public var detents: [PhoneWritingSheetDetent]
  public var minimumActionTarget: Double

  public init(detents: [PhoneWritingSheetDetent], minimumActionTarget: Double) {
    self.detents = detents
    self.minimumActionTarget = minimumActionTarget
  }
}

/// Pure inventory for writing sheets. SwiftUI consumes this plan so sheet sizing
/// and action targets do not drift between otherwise equivalent destinations.
public struct PhoneWritingSheetPolicy: Equatable, Sendable {
  public var metrics: PhoneWritingLayoutMetrics

  public init(metrics: PhoneWritingLayoutMetrics = .standard) {
    self.metrics = metrics
  }

  public func plan(for kind: PhoneWritingSheetKind) -> PhoneWritingSheetPlan {
    let detents: [PhoneWritingSheetDetent]
    switch kind {
    case .split:
      detents = [.medium, .large]
    case .scenes, .ideaBoard, .link, .image:
      detents = [.medium]
    }
    return PhoneWritingSheetPlan(
      detents: detents, minimumActionTarget: metrics.minimumHitTarget)
  }
}

/// Pure phone writing chrome policy. UIKit and SwiftUI consume this inventory;
/// neither platform view is allowed to silently invent or lose an action.
public struct PhoneWritingChromePolicy: Equatable, Sendable {
  public var metrics: PhoneWritingLayoutMetrics
  public var writingChrome: WritingChromePolicy

  public init(
    metrics: PhoneWritingLayoutMetrics = .standard,
    writingChrome: WritingChromePolicy = WritingChromePolicy()
  ) {
    self.metrics = metrics
    self.writingChrome = writingChrome
  }

  public func plan(
    kind: PaneLayoutKind, fullWidth: Bool = false, hasSelection: Bool = false
  ) -> PhoneWritingChromePlan {
    let base = writingChrome.plan(
      kind: kind, fullWidth: fullWidth, hasSelection: hasSelection)
    return PhoneWritingChromePlan(
      menuGroups: base.menuGroups,
      selectionActions: base.selectionActions,
      splitKinds: [.text, .screenplay, .presentation, .agent],
      headerHeight: metrics.minimumHitTarget,
      minimumActionTarget: metrics.minimumHitTarget)
  }

  public func duration(
    for transition: PhoneWritingChromeTransition, reduceMotion: Bool
  ) -> Double {
    guard !reduceMotion else { return 0 }
    switch transition {
    case .dim: return writingChrome.metrics.dimDuration
    case .reveal: return writingChrome.metrics.revealDuration
    }
  }
}

public struct PhoneScreenplayElementOverlayPolicy: Equatable, Sendable {
  public var minimumTarget: Double
  public var clearance: Double

  public init(minimumTarget: Double, clearance: Double) {
    self.minimumTarget = minimumTarget
    self.clearance = clearance
  }

  public func caretRectInScroll(
    caretRect: PaneLayoutRect, pageOrigin: PaneLayoutPoint, zoomScale: Double
  ) -> PaneLayoutRect {
    let scale = max(0, zoomScale)
    return PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: pageOrigin.x + caretRect.origin.x * scale,
        y: pageOrigin.y + caretRect.origin.y * scale),
      size: PaneLayoutSize(
        width: caretRect.size.width * scale,
        height: caretRect.size.height * scale))
  }

  public func buttonRect(
    for caretRect: PaneLayoutRect, visibleRect: PaneLayoutRect
  ) -> PaneLayoutRect? {
    guard intersects(caretRect, visibleRect) else { return nil }
    let size = min(minimumTarget, visibleRect.size.width, visibleRect.size.height)
    let x = clamp(
      caretRect.maxX + clearance,
      minimum: visibleRect.origin.x,
      maximum: visibleRect.maxX - size)
    let y = clamp(
      caretRect.origin.y + (caretRect.size.height - size) / 2,
      minimum: visibleRect.origin.y,
      maximum: visibleRect.maxY - size)
    return PaneLayoutRect(
      origin: PaneLayoutPoint(x: x, y: y),
      size: PaneLayoutSize(width: size, height: size))
  }

  private func intersects(_ first: PaneLayoutRect, _ second: PaneLayoutRect) -> Bool {
    first.maxX > second.origin.x && first.origin.x < second.maxX
      && first.maxY > second.origin.y && first.origin.y < second.maxY
  }

  private func clamp(_ value: Double, minimum: Double, maximum: Double) -> Double {
    guard maximum >= minimum else { return minimum }
    return min(max(value, minimum), maximum)
  }
}
