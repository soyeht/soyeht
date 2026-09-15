import Foundation
import SoyehtProtocol

public enum HarnessPickerGroup: Int, CaseIterable, Sendable {
  case ready
  case needsSetup
  case unavailable

  public var title: String {
    switch self {
    case .ready: "Ready"
    case .needsSetup: "Needs setup"
    case .unavailable: "Not installed"
    }
  }
}

public struct HarnessPickerSection: Sendable {
  public let group: HarnessPickerGroup
  public let items: [HarnessInfo]

  public init(group: HarnessPickerGroup, items: [HarnessInfo]) {
    self.group = group
    self.items = items
  }
}

public enum HarnessPickerModel {
  public static func sections(
    _ harnesses: [HarnessInfo], query: String = ""
  ) -> [HarnessPickerSection] {
    let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
    let filtered = harnesses.filter { harness in
      needle.isEmpty || harness.name.localizedCaseInsensitiveContains(needle)
        || harness.id.localizedCaseInsensitiveContains(needle)
    }
    return HarnessPickerGroup.allCases.compactMap { group in
      let items = filtered.filter { pickerGroup(for: $0) == group }.sorted(by: ordered)
      return items.isEmpty ? nil : HarnessPickerSection(group: group, items: items)
    }
  }

  public static func defaultHarness(
    in harnesses: [HarnessInfo], excluding excludedId: String? = nil
  ) -> HarnessInfo? {
    sections(harnesses)
      .filter { $0.group != .unavailable }
      .flatMap(\.items)
      .first { $0.id != excludedId }
  }

  public static func setupState(for harness: HarnessInfo) -> HarnessSetupState {
    if let state = harness.setupState { return state }
    if !harness.available { return .not_installed }
    if harness.capabilities.contains(where: { $0.result == .unauthenticated }) {
      return .sign_in
    }
    if harness.capabilities.contains(where: { $0.result == .inconclusive }) {
      return .check_setup
    }
    return .ready
  }

  public static func pickerGroup(for harness: HarnessInfo) -> HarnessPickerGroup {
    switch setupState(for: harness) {
    case .ready: .ready
    case .sign_in, .check_setup: .needsSetup
    case .not_installed: .unavailable
    }
  }

  public static func statusLabel(for harness: HarnessInfo) -> String {
    switch setupState(for: harness) {
    case .ready: "Ready"
    case .sign_in: "Sign in"
    case .check_setup: "Check setup"
    case .not_installed: "Not installed"
    }
  }

  private static func ordered(_ lhs: HarnessInfo, _ rhs: HarnessInfo) -> Bool {
    let leftName = lhs.name.lowercased()
    let rightName = rhs.name.lowercased()
    return leftName == rightName ? lhs.id < rhs.id : leftName < rightName
  }
}
