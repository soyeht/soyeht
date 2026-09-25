import Foundation
import SoyehtProtocol

public enum HarnessPickerGroup: Int, CaseIterable, Sendable {
  case ready
  case notChecked
  case signIn
  case checkSetup
  case unavailable

  public var title: String {
    switch self {
    case .ready: "Ready"
    case .notChecked: "Not checked"
    case .signIn: "Sign in"
    case .checkSetup: "Check setup"
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

public enum HarnessPickerContentState: Sendable, Equatable {
  case loading
  case emptyCatalogue
  case noMatches
  case results
}

public enum ModelPickerContentState: Sendable, Equatable {
  case loading
  case unavailable
  case empty
  case results
}

public enum HarnessPickerModel {
  public static let preferredHarnessKey = "soyeht.harness.preferred"

  public static func contentState(
    harnesses: [HarnessInfo], query: String, loading: Bool
  ) -> HarnessPickerContentState {
    if loading && harnesses.isEmpty { return .loading }
    if harnesses.isEmpty { return .emptyCatalogue }
    return sections(harnesses, query: query).isEmpty ? .noMatches : .results
  }

  public static func modelContentState(
    catalogue: AgentCatalogue?, loading: Bool
  ) -> ModelPickerContentState {
    if loading && catalogue == nil { return .loading }
    guard let catalogue else { return .unavailable }
    return catalogue.models.isEmpty ? .empty : .results
  }

  public static func modelLabel(for model: ModelInfo) -> String {
    model.observedRefusal == nil ? model.name : "\(model.name) — refused"
  }

  public static func modelRefusalReason(for model: ModelInfo?) -> String? {
    model?.observedRefusal?.reason
  }

  public static func sections(
    _ harnesses: [HarnessInfo], query: String = ""
  ) -> [HarnessPickerSection] {
    let filtered = harnesses.filter { matches($0, query: query) }
    return HarnessPickerGroup.allCases.compactMap { group in
      let items = filtered.filter { pickerGroup(for: $0) == group }.sorted(by: ordered)
      return items.isEmpty ? nil : HarnessPickerSection(group: group, items: items)
    }
  }

  public static func matches(_ harness: HarnessInfo, query: String) -> Bool {
    let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
    return needle.isEmpty || harness.name.localizedCaseInsensitiveContains(needle)
      || harness.id.localizedCaseInsensitiveContains(needle)
  }

  public static func defaultHarness(
    in harnesses: [HarnessInfo], excluding excludedId: String? = nil,
    preferredId: String? = nil
  ) -> HarnessInfo? {
    let candidates = harnesses.filter { $0.available && $0.id != excludedId }
    if let preferredId, let preferred = candidates.first(where: { $0.id == preferredId }) {
      return preferred
    }
    return candidates.filter { setupState(for: $0) == .ready }.sorted {
      let left = Int64($0.lastReadyAtMs ?? "") ?? 0
      let right = Int64($1.lastReadyAtMs ?? "") ?? 0
      return left == right ? ordered($0, $1) : left > right
    }.first
  }

  public static func setupState(for harness: HarnessInfo) -> HarnessSetupState {
    if let state = harness.setupState { return state }
    if !harness.available { return .not_installed }
    if harness.capabilities.contains(where: { $0.result == .unauthenticated }) {
      return .sign_in
    }
    return .not_checked
  }

  public static func pickerGroup(for harness: HarnessInfo) -> HarnessPickerGroup {
    switch setupState(for: harness) {
    case .ready: return .ready
    case .not_checked: return .notChecked
    case .sign_in: return .signIn
    case .check_setup: return .checkSetup
    case .not_installed: return .unavailable
    }
  }

  public static func statusLabel(for harness: HarnessInfo) -> String {
    switch setupState(for: harness) {
    case .ready: "Ready"
    case .not_checked: "Not checked"
    case .sign_in: "Sign in"
    case .check_setup: "Check setup"
    case .not_installed: "Not installed"
    }
  }

  public static func statusReason(for harness: HarnessInfo) -> String {
    switch harness.setupReason {
    case .never_started: "Installed; this agent has not been started yet. You can start it now."
    case .executable_missing: "Executable not found on the engine's launch PATH."
    case .registry_package_unverified:
      "The adapter runner is available; its package has not been verified locally. Starting it will check the package."
    case .authentication_required: "This agent requires sign-in before it can start."
    case .session_failed: "The agent started, but its session failed. Try starting it again."
    case .launch_failed: "The agent could not start. Check its installation and try again."
    case .environment_unavailable: "The engine could not read the launch environment."
    case nil:
      switch setupState(for: harness) {
      case .ready: "An agent session opened successfully."
      case .not_checked: "Installed; this agent has not been started yet. You can start it now."
      case .sign_in: "This agent requires sign-in before it can start."
      case .check_setup: "The last attempt failed. Check the agent's installation."
      case .not_installed: "Executable not found on the engine's launch PATH."
      }
    }
  }

  private static func ordered(_ lhs: HarnessInfo, _ rhs: HarnessInfo) -> Bool {
    let leftName = lhs.name.lowercased()
    let rightName = rhs.name.lowercased()
    return leftName == rightName ? lhs.id < rhs.id : leftName < rightName
  }
}
