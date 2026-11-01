import Foundation
import SoyehtProtocol
import SwiftUI

/// What the detail of one provider's usage says: the Mac balloon and the iPhone sheet both
/// draw it, each in its own look, so the branches, the wording and the numbers live here once.
public struct UsageDetailModel: Sendable {
  /// A line instead of the windows: the reading cannot be shown, and why.
  public struct Notice: Sendable, Equatable {
    public let message: String
    /// The SF Symbol drawn before the message.
    public let symbol: String
  }

  /// One usage window with the numbers its bar and labels show.
  public struct Row: Sendable, Equatable {
    public let label: String
    public let fraction: Double
    public let percentage: String
    public let reset: String
    public let band: UsageBand
    /// A last-known reading draws its bar muted.
    public let isStale: Bool
    public var barOpacity: Double { isStale ? UsageEmphasis.lastKnown.arcOpacity : 1 }
  }

  public enum Content: Sendable, Equatable {
    case notice(Notice)
    /// The windows and, under them, why a last-known reading was not replaced.
    case windows([Row], hint: String?)
  }

  public let providerName: String
  /// Whether the reading is current: a last-known reading and no reading look alike.
  public let isCurrent: Bool
  /// When the window the ring follows resets.
  public let reset: String
  public let content: Content
  public let footer: String

  /// The status dot's palette role, the same on every platform.
  public func statusColor(in colors: SoyehtThemeColorSet) -> Color {
    isCurrent ? colors.success : colors.muted
  }

  /// `onThisMac` is false on a phone or tablet, where granting access means going to the Mac.
  public init(_ reading: UsageReading, onThisMac: Bool) {
    let presentation = UsagePresentation(reading)
    providerName = reading.provider.displayName
    isCurrent = reading.state == .ok
    footer = "Source: \(reading.source.displayName) · \(UsageTimeText.age(of: reading))"
    if let line = Self.activityLine(reading) {
      // A count of what the device recorded: no window, so no percentage and no bar.
      reset = "No plan limit"
      content = .notice(
        Notice(
          message: "\(line). No plan limit is known, so there is no percentage."
            + (presentation.staleHint.map { " \($0)" } ?? ""),
          symbol: "chart.bar"))
      return
    }
    reset = UsageTimeText.reset(presentation.displayedWindow?.resetsAt)
    let isStale = reading.state == .stale
    switch (reading.state, reading.primary) {
    case (.login_needed, _):
      content = .notice(
        Notice(
          message: "Sign in in your terminal.", symbol: "person.crop.circle.badge.exclamationmark"))
    case (.no_access, _):
      content = .notice(
        Notice(
          message: onThisMac
            ? "Allow access in Settings." : "Allow access in Settings on your Mac.",
          symbol: "lock"))
    case (_, let primary?):
      let rows = [primary, reading.secondary].compactMap { $0 }.map {
        Row(window: $0, isStale: isStale)
      }
      content = .windows(rows, hint: presentation.staleHint)
    default:
      let message =
        switch reading.state {
        case .ok: "No reading yet."
        case .stale: "Last reading is stale."
        case .login_needed, .no_access, .unavailable: presentation.unavailableMessage
        }
      content = .notice(Notice(message: message, symbol: "exclamationmark.circle"))
    }
  }

  /// The line for a reading that holds an activity and no window, current or last known.
  private static func activityLine(_ reading: UsageReading) -> String? {
    guard reading.primary == nil, reading.state == .ok || reading.state == .stale,
      let activity = reading.activity
    else { return nil }
    return UsageActivityText.line(activity)
  }
}

extension UsageDetailModel.Row {
  fileprivate init(window: UsageWindow, isStale: Bool) {
    let fraction = min(max(window.used, 0), 1)
    label = window.label
    self.fraction = fraction
    percentage = "\(Int((fraction * 100).rounded()))%"
    reset = UsageTimeText.reset(window.resetsAt)
    band = UsageBand(usedFraction: fraction)
    self.isStale = isStale
  }
}
