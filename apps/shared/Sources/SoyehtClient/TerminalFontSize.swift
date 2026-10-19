import Foundation
import Observation

/// The terminal text size rule: the default and the limits are tokens of
/// `contracts/design-tokens.json` (`paneLayout`), so every client that zooms a terminal
/// stays between the same bounds and moves by the same step.
public enum TerminalFontSizePolicy {
  public static let defaultSize = PaneLayoutContentMetrics.terminalFontPointSize
  public static let minimum = Double(DesignTokens.PaneLayout.terminalFontSizeMinimum)
  public static let maximum = Double(DesignTokens.PaneLayout.terminalFontSizeMaximum)
  public static let step = Double(DesignTokens.PaneLayout.terminalFontSizeStep)

  /// The nearest allowed size: a whole number of steps, inside the limits. A size that is not
  /// a number falls back to the default.
  public static func clamped(_ size: Double) -> Double {
    guard size.isFinite else { return defaultSize }
    return min(maximum, max(minimum, (size / step).rounded() * step))
  }

  /// `steps` whole steps larger (positive) or smaller (negative) than `size`.
  public static func stepped(_ size: Double, by steps: Int) -> Double {
    clamped(clamped(size) + Double(steps) * step)
  }

  /// The size a pinch has reached: `base` times the gesture's scale. A scale that is not a
  /// positive number leaves the size alone.
  public static func pinched(from base: Double, scale: Double) -> Double {
    guard scale.isFinite, scale > 0 else { return clamped(base) }
    return clamped(base * scale)
  }
}

/// A command a person gives to change the terminal text size, whatever the input: a pinch,
/// a hardware key or a menu.
public enum TerminalZoomCommand: Equatable, Sendable {
  case larger
  case smaller
  case reset
}

/// A hardware key that zooms the terminal while Command is held. `input` is the character
/// the key produces; "+" is the shifted key that shares a cap with "=".
public struct TerminalZoomShortcut: Equatable, Sendable {
  public let input: String
  public let shift: Bool
  public let command: TerminalZoomCommand

  public static let all = [
    TerminalZoomShortcut(input: "=", shift: false, command: .larger),
    TerminalZoomShortcut(input: "+", shift: true, command: .larger),
    TerminalZoomShortcut(input: "-", shift: false, command: .smaller),
    TerminalZoomShortcut(input: "0", shift: false, command: .reset),
  ]

  public static func matching(input: String, shift: Bool) -> TerminalZoomShortcut? {
    all.first { $0.input == input && $0.shift == shift }
  }
}

/// The terminal text size of this device, kept between launches. One size for every
/// terminal pane.
@MainActor @Observable
public final class TerminalFontSizeStore {
  public static let sizeKey = "soyeht.terminal.fontSize"
  public static let shared = TerminalFontSizeStore()

  public private(set) var size: Double
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() { self.init(preferences: UserDefaultsClientPreferenceStore()) }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    size =
      preferences.integer(forKey: Self.sizeKey).map { TerminalFontSizePolicy.clamped(Double($0)) }
      ?? TerminalFontSizePolicy.defaultSize
  }

  public var canGrow: Bool { size < TerminalFontSizePolicy.maximum }
  public var canShrink: Bool { size > TerminalFontSizePolicy.minimum }

  public func apply(_ command: TerminalZoomCommand) {
    switch command {
    case .larger: set(TerminalFontSizePolicy.stepped(size, by: 1))
    case .smaller: set(TerminalFontSizePolicy.stepped(size, by: -1))
    case .reset: set(TerminalFontSizePolicy.defaultSize)
    }
  }

  /// A pinch passes `persisting: false` while it moves and calls `commit()` when it ends, so
  /// the size is saved once.
  public func set(_ requested: Double, persisting: Bool = true) {
    let next = TerminalFontSizePolicy.clamped(requested)
    guard next != size else { return }
    size = next
    if persisting { commit() }
  }

  public func commit() { preferences.set(Int(size.rounded()), forKey: Self.sizeKey) }
}
