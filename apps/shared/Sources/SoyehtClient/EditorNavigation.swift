import Foundation

public struct DoubleShiftDetector: Sendable {
  private let maximumInterval: TimeInterval
  private var previousDown: TimeInterval?

  public init(maximumInterval: TimeInterval = 0.35) {
    self.maximumInterval = maximumInterval
  }

  public mutating func registerShiftDown(at timestamp: TimeInterval) -> Bool {
    guard let previousDown else {
      self.previousDown = timestamp
      return false
    }
    let interval = timestamp - previousDown
    if interval >= 0, interval <= maximumInterval {
      self.previousDown = nil
      return true
    }
    self.previousDown = timestamp
    return false
  }
}
