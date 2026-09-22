public enum WritingSlideShowInput: Equatable, Sendable {
  case rightArrow
  case leftArrow
  case pageDown
  case pageUp
  case space
  case click
  case escape
  case other
}

public enum WritingSlideShowCommand: Equatable, Sendable {
  case advance
  case previous
  case exit
}

public struct WritingSlideShowPolicy: Equatable, Sendable {
  public static let standard = WritingSlideShowPolicy()

  public let aspectRatio: Double
  public let cursorIdleDelay: Double
  public let fullScreenEntryTimeout: Double
  public let slideSizing: WritingPresentedSlideSizing

  public init(
    aspectRatio: Double = 16.0 / 9.0,
    cursorIdleDelay: Double = 2,
    fullScreenEntryTimeout: Double = 5,
    slideSizing: WritingPresentedSlideSizing = .standard
  ) {
    self.aspectRatio = aspectRatio
    self.cursorIdleDelay = cursorIdleDelay
    self.fullScreenEntryTimeout = fullScreenEntryTimeout
    self.slideSizing = slideSizing
  }

  public func command(
    for input: WritingSlideShowInput, commandModified: Bool = false
  ) -> WritingSlideShowCommand? {
    guard !commandModified else { return nil }
    return switch input {
    case .rightArrow, .pageDown, .space, .click:
      .advance
    case .leftArrow, .pageUp:
      .previous
    case .escape:
      .exit
    case .other:
      nil
    }
  }

  public func fittedStage(in viewport: PaneLayoutSize) -> PaneLayoutRect {
    guard viewport.width.isFinite, viewport.height.isFinite,
      viewport.width > 0, viewport.height > 0,
      aspectRatio.isFinite, aspectRatio > 0
    else { return PaneLayoutRect(size: PaneLayoutSize(width: 0, height: 0)) }

    let width = min(viewport.width, viewport.height * aspectRatio)
    let height = width / aspectRatio
    return PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: (viewport.width - width) / 2,
        y: (viewport.height - height) / 2),
      size: PaneLayoutSize(width: width, height: height))
  }
}

public struct WritingPresentedSlideMetrics: Equatable, Sendable {
  public let titleSize: Double
  public let contentSpacing: Double
  public let padding: Double
}

public struct WritingPresentedSlideSizing: Equatable, Sendable {
  public static let standard = WritingPresentedSlideSizing(
    titleMinimum: 32, titleMaximum: 72, titleWidthDivisor: 18,
    spacingMinimum: 18, spacingMaximum: 40, spacingHeightDivisor: 18,
    paddingMinimum: 28, paddingMaximum: 72, paddingWidthDivisor: 24)

  public let titleMinimum: Double
  public let titleMaximum: Double
  public let titleWidthDivisor: Double
  public let spacingMinimum: Double
  public let spacingMaximum: Double
  public let spacingHeightDivisor: Double
  public let paddingMinimum: Double
  public let paddingMaximum: Double
  public let paddingWidthDivisor: Double

  public func metrics(in viewport: PaneLayoutSize) -> WritingPresentedSlideMetrics {
    WritingPresentedSlideMetrics(
      titleSize: Self.clamp(
        viewport.width / titleWidthDivisor, minimum: titleMinimum, maximum: titleMaximum),
      contentSpacing: Self.clamp(
        viewport.height / spacingHeightDivisor,
        minimum: spacingMinimum, maximum: spacingMaximum),
      padding: Self.clamp(
        viewport.width / paddingWidthDivisor,
        minimum: paddingMinimum, maximum: paddingMaximum))
  }

  private static func clamp(_ value: Double, minimum: Double, maximum: Double) -> Double {
    min(maximum, max(minimum, value))
  }
}

public struct WritingSlideShowState: Equatable, Sendable {
  public private(set) var index: Int
  public private(set) var slideCount: Int

  public init(index: Int, slideCount: Int) {
    self.slideCount = max(0, slideCount)
    self.index = Self.clamped(index, slideCount: self.slideCount)
  }

  /// Returns true when no slide remains and the presentation must close.
  @discardableResult
  public mutating func reconcile(slideCount: Int) -> Bool {
    self.slideCount = max(0, slideCount)
    index = Self.clamped(index, slideCount: self.slideCount)
    return self.slideCount == 0
  }

  /// Returns true when the command asks the presentation to close.
  @discardableResult
  public mutating func apply(_ command: WritingSlideShowCommand) -> Bool {
    switch command {
    case .advance:
      index = Self.clamped(index + 1, slideCount: slideCount)
      return false
    case .previous:
      index = Self.clamped(index - 1, slideCount: slideCount)
      return false
    case .exit:
      return true
    }
  }

  private static func clamped(_ index: Int, slideCount: Int) -> Int {
    min(max(0, index), max(0, slideCount - 1))
  }
}

public enum WritingSlideShowCursorEvent: Equatable, Sendable {
  case activity
  case idleExpired
  case sessionEnded
}

public enum WritingSlideShowCursorEffect: Equatable, Sendable {
  case none
  case hide
  case show
}

public struct WritingSlideShowCursorState: Equatable, Sendable {
  public private(set) var isHidden = false

  public init() {}

  public mutating func reduce(
    _ event: WritingSlideShowCursorEvent
  ) -> WritingSlideShowCursorEffect {
    switch event {
    case .activity:
      guard isHidden else { return .none }
      isHidden = false
      return .show
    case .idleExpired:
      guard !isHidden else { return .none }
      isHidden = true
      return .hide
    case .sessionEnded:
      guard isHidden else { return .none }
      isHidden = false
      return .show
    }
  }
}

public enum WritingSlideShowWindowPhase: Equatable, Sendable {
  case inactive
  case enteringFullScreen
  case active
  case exitingFullScreen
}

public enum WritingSlideShowWindowEffect: Equatable, Sendable {
  case none
  case enterFullScreen
  case exitFullScreen
  case activate
  case finish
}

public struct WritingSlideShowWindowState: Equatable, Sendable {
  public private(set) var phase: WritingSlideShowWindowPhase = .inactive
  public private(set) var startedFullScreen = false
  private var exitPending = false

  public init() {}

  public mutating func begin(
    isFullScreen: Bool, canEnterFullScreen: Bool = true
  ) -> WritingSlideShowWindowEffect {
    guard phase == .inactive else { return .none }
    startedFullScreen = isFullScreen
    exitPending = false
    if isFullScreen {
      phase = .active
      return .activate
    }
    guard canEnterFullScreen else { return .finish }
    phase = .enteringFullScreen
    return .enterFullScreen
  }

  public mutating func requestExit() -> WritingSlideShowWindowEffect {
    switch phase {
    case .inactive, .exitingFullScreen:
      return .none
    case .enteringFullScreen:
      exitPending = true
      return .none
    case .active:
      if startedFullScreen {
        phase = .inactive
        return .finish
      }
      phase = .exitingFullScreen
      return .exitFullScreen
    }
  }

  public mutating func didEnterFullScreen() -> WritingSlideShowWindowEffect {
    guard phase == .enteringFullScreen else { return .none }
    if exitPending {
      phase = .exitingFullScreen
      return .exitFullScreen
    }
    phase = .active
    return .activate
  }

  public mutating func didExitFullScreen() -> WritingSlideShowWindowEffect {
    guard phase != .inactive else { return .none }
    phase = .inactive
    return .finish
  }

  public mutating func didFailToEnterFullScreen() -> WritingSlideShowWindowEffect {
    guard phase == .enteringFullScreen else { return .none }
    phase = .inactive
    return .finish
  }
}
