import Foundation
import Observation
import SoyehtProtocol
import SwiftUI

public enum WebLinkDestination: Equatable, Sendable {
  case soyeht(URL)
  case system(URL)
}

public enum WebLinkRouting {
  private static let maximumLinks = 128
  static let matcher = try? NSRegularExpression(
    pattern: #"(?i)https?://[^\s<>\"']+"#)

  public static func destination(
    for value: String, openExternalLinksInSoyeht: Bool = false
  ) -> WebLinkDestination? {
    guard let url = normalized(value), let host = url.host?.lowercased() else { return nil }
    let local = host == "localhost" || host == "127.0.0.1" || host == "::1"
    return local || openExternalLinksInSoyeht ? .soyeht(url) : .system(url)
  }

  public static func links(in text: String) -> [(
    label: String, url: URL, range: Range<String.Index>
  )] {
    guard let matcher else { return [] }
    let whole = NSRange(text.startIndex..<text.endIndex, in: text)
    return matcher.matches(in: text, range: whole).prefix(maximumLinks).compactMap { match in
      guard var range = Range(match.range, in: text) else { return nil }
      while range.lowerBound < range.upperBound,
        let last = text[range].last, ".,;:!?)]}".contains(last)
      {
        range = range.lowerBound..<text.index(before: range.upperBound)
      }
      let label = String(text[range])
      guard let url = normalized(label) else { return nil }
      return (label, url, range)
    }
  }

  private static func normalized(_ value: String) -> URL? {
    guard value.utf8.count <= ProtocolLimits.webFieldBytes,
      let components = URLComponents(string: value),
      let scheme = components.scheme?.lowercased(), ["http", "https"].contains(scheme),
      components.host?.isEmpty == false, components.user == nil, components.password == nil,
      let url = components.url
    else { return nil }
    return url
  }
}

@MainActor @Observable
public final class WebLinkPreferences {
  public static let openExternalKey = "soyeht.webLinks.openExternal"
  public static let shared = WebLinkPreferences()
  public private(set) var openExternalLinksInSoyeht: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() { self.init(preferences: UserDefaultsClientPreferenceStore()) }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    openExternalLinksInSoyeht = preferences.bool(forKey: Self.openExternalKey)
  }

  public func setOpenExternalLinksInSoyeht(_ enabled: Bool) {
    openExternalLinksInSoyeht = enabled
    preferences.set(enabled, forKey: Self.openExternalKey)
  }

  public func destination(for value: String) -> WebLinkDestination? {
    WebLinkRouting.destination(
      for: value, openExternalLinksInSoyeht: openExternalLinksInSoyeht)
  }
}

public struct WebLinkOpenAction: Sendable {
  private let handler: (@MainActor @Sendable (String, URL) -> Void)?

  public init(_ handler: @escaping @MainActor @Sendable (String, URL) -> Void) {
    self.handler = handler
  }

  fileprivate init() { handler = nil }
  var enabled: Bool { handler != nil }

  @MainActor public func callAsFunction(workspaceId: String, url: URL) {
    handler?(workspaceId, url)
  }
}

private struct WebLinkOpenActionKey: EnvironmentKey {
  static let defaultValue = WebLinkOpenAction()
}

extension EnvironmentValues {
  public var webLinkOpenAction: WebLinkOpenAction {
    get { self[WebLinkOpenActionKey.self] }
    set { self[WebLinkOpenActionKey.self] = newValue }
  }
}
