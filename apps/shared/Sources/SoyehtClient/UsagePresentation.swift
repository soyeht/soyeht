import Foundation
import SoyehtProtocol
import SwiftUI

/// The colour band of a used fraction. This is the only place the thresholds live: every ring,
/// bar and dot on the Mac, iPhone and iPad asks it (contract 10 §2).
public enum UsageBand: Sendable, Equatable {
  /// Below 50 %.
  case success
  /// From 50 % to 80 %, both included.
  case warning
  /// Above 80 %.
  case danger

  public init(usedFraction: Double) {
    let fraction = min(max(usedFraction, 0), 1)
    if fraction < 0.5 {
      self = .success
    } else if fraction <= 0.8 {
      self = .warning
    } else {
      self = .danger
    }
  }

  /// The palette role of the band. Usage never introduces a colour of its own.
  @MainActor public var color: Color {
    switch self {
    case .success: SoyehtThemeColors.success
    case .warning: SoyehtThemeColors.warning
    case .danger: SoyehtThemeColors.danger
    }
  }
}

/// How far a reading can be trusted at a glance (contract 10 §2, "Ring states").
public enum UsageEmphasis: Sendable, Equatable {
  /// A reading inside the 15-minute freshness window: the band colour at full strength.
  case current
  /// A stale reading, older than 15 minutes or restored after a restart and not yet checked
  /// again: the percentage stays and the arc keeps its band colour, muted.
  case lastKnown
  /// No reading (unchecked, unavailable, sign-in needed, no access): a dimmed ring and "—".
  case none

  /// Opacity of the band-coloured arc.
  public var arcOpacity: Double {
    switch self {
    case .current: 1
    case .lastKnown: 0.45
    case .none: 0
    }
  }

  /// Opacity of the whole ring, glyph and label included.
  public var ringOpacity: Double { self == .none ? 0.46 : 1 }
}

public enum UsageTimestamp {
  public static func date(from value: String) -> Date? {
    let fractional = ISO8601DateFormatter()
    fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value)
  }
}

public struct UsagePresentation: Sendable {
  public let reading: UsageReading

  public init(_ reading: UsageReading) {
    self.reading = reading
  }

  public var displayedWindow: UsageWindow? {
    switch reading.provider {
    case .claude, .codex:
      weeklyWindow ?? reading.secondary ?? reading.primary
    case .cursor:
      reading.primary
    }
  }

  private var weeklyWindow: UsageWindow? {
    [reading.primary, reading.secondary]
      .compactMap { $0 }
      .first { $0.label.lowercased().contains("week") }
  }

  public var fraction: Double? {
    guard reading.state == .ok || reading.state == .stale,
      let used = displayedWindow?.used
    else { return nil }
    return min(max(used, 0), 1)
  }

  public var isStale: Bool { reading.state == .stale }

  /// The band of the displayed window. A stale reading keeps its last known band.
  public var band: UsageBand? { fraction.map(UsageBand.init(usedFraction:)) }

  public var emphasis: UsageEmphasis {
    guard fraction != nil else { return .none }
    return isStale ? .lastKnown : .current
  }

  public var unavailableMessage: String {
    Self.message(for: reading.unavailableReason, provider: reading.provider)
      ?? "Usage is unavailable."
  }

  /// Why a last-known reading was not replaced: the latest refresh failed for this reason.
  public var staleHint: String? {
    guard isStale,
      let reason = Self.message(for: reading.unavailableReason, provider: reading.provider)
    else { return nil }
    return "Latest refresh: \(reason)"
  }

  private static func message(
    for reason: UsageUnavailableReason?, provider: UsageProvider
  ) -> String? {
    switch reason {
    case .not_checked: "Usage has not been checked yet."
    case .tool_not_found: "\(provider.displayName) not found on this Mac."
    case .app_server_failed: "\(provider.displayName) app server could not provide usage."
    case .invalid_response: "Usage reader returned invalid data."
    case .rate_limited: "\(provider.displayName) is limiting usage requests; retrying later."
    case .network_error: "\(provider.displayName) could not be reached."
    case .login_needed: "Sign in to \(provider.displayName) in your terminal."
    case .no_access: "Access to the \(provider.displayName) login was declined."
    case .source_failed: "\(provider.displayName) could not provide usage."
    case nil: nil
    }
  }

  public var percentage: String {
    guard let fraction else { return "—" }
    return "\(Int((fraction * 100).rounded()))%"
  }
}

/// The band-coloured arc of a usage ring, drawn the same way on every device.
public struct UsageRingArc: View {
  private let presentation: UsagePresentation
  private let lineWidth: CGFloat

  public init(_ presentation: UsagePresentation, lineWidth: CGFloat) {
    self.presentation = presentation
    self.lineWidth = lineWidth
  }

  public var body: some View {
    if let fraction = presentation.fraction, let band = presentation.band {
      Circle()
        .trim(from: 0, to: fraction)
        .stroke(band.color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
        .rotationEffect(.degrees(-90))
        .opacity(presentation.emphasis.arcOpacity)
    }
  }
}

public struct ProviderGlyph: View {
  private let provider: UsageProvider

  public init(_ provider: UsageProvider) {
    self.provider = provider
  }

  public var body: some View {
    Image(provider.resourceName, bundle: .module)
      .resizable()
      .renderingMode(.template)
      .aspectRatio(contentMode: .fit)
  }
}

extension UsageProvider {
  fileprivate var resourceName: String {
    switch self {
    case .claude: "claude"
    case .codex: "codex"
    case .cursor: "cursor"
    }
  }
}

/// Relative times of usage readings, shared by the Mac balloon and the iPhone detail.
public enum UsageTimeText {
  public static func reset(_ value: String?) -> String {
    guard let value, let date = UsageTimestamp.date(from: value) else {
      return "Reset unavailable"
    }
    let seconds = date.timeIntervalSinceNow
    guard seconds > 0 else { return "Reset due" }
    return "Resets in \(duration(seconds))"
  }

  public static func fetched(_ value: String) -> String {
    guard let date = UsageTimestamp.date(from: value) else { return "time unavailable" }
    let seconds = max(0, Date().timeIntervalSince(date))
    if seconds < 60 { return "updated just now" }
    return "updated \(duration(seconds)) ago"
  }

  public static func asOf(_ value: String) -> String {
    guard let date = UsageTimestamp.date(from: value) else { return "time unavailable" }
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_US_POSIX")
    formatter.dateFormat = "HH:mm"
    return "as of \(formatter.string(from: date))"
  }

  /// "updated … ago" for a current reading, "as of HH:mm" for a stale one.
  public static func age(of reading: UsageReading) -> String {
    reading.state == .stale ? asOf(reading.fetchedAt) : fetched(reading.fetchedAt)
  }

  static func duration(_ seconds: TimeInterval) -> String {
    let minutes = max(1, Int(seconds) / 60)
    if minutes < 60 { return "\(minutes)m" }
    let hours = minutes / 60
    let remainder = minutes % 60
    if hours < 24 { return remainder == 0 ? "\(hours)h" : "\(hours)h \(remainder)m" }
    return "\(hours / 24)d \(hours % 24)h"
  }
}
