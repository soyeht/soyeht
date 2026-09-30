import Foundation
import SoyehtProtocol

extension ConfigOption {
  /// The harness's own permission-mode selector (ADR 0070).
  public var isPermissionMode: Bool { category == "mode" || id == "mode" }
}

/// One permission mode exactly as the harness announced it.
public struct AgentPermissionMode: Equatable, Sendable, Identifiable {
  public let id: String
  public let name: String
  public let detail: String?
  /// Approves every action without asking. Only an explicit menu choice selects it.
  public let fullAccess: Bool
}

/// The modes a harness announced, the one it confirmed and the one the engine
/// is still applying. Nothing here maps modes to a Soyeht vocabulary.
public struct AgentPermissionModes: Equatable, Sendable {
  static let fullAccessKind = "full_access"
  public static let defaultModeKeyPrefix = "harness.defaultMode."

  public let configId: String
  public let modes: [AgentPermissionMode]
  public let current: String?
  public let pending: String?

  public init?(options: [ConfigOption], operation: ConfigOperation? = nil) {
    guard let option = options.first(where: \.isPermissionMode), option.type == .select,
      !option.options.isEmpty
    else { return nil }
    configId = option.id
    modes = option.options.map {
      AgentPermissionMode(
        id: $0.value, name: $0.name, detail: $0.description,
        fullAccess: $0.kind == Self.fullAccessKind)
    }
    if case .v0(let value) = option.currentValue { current = value } else { current = nil }
    if let operation, operation.configId == option.id, case .v0(let value) = operation.value {
      pending = value
    } else {
      pending = nil
    }
  }

  public init?(agent: AgentSession) {
    self.init(options: agent.options, operation: agent.operation)
  }

  public func mode(_ id: String?) -> AgentPermissionMode? { modes.first { $0.id == id } }

  /// The confirmed mode; a pending one is shown only as pending.
  public var currentMode: AgentPermissionMode? { mode(current) }
  public var pendingMode: AgentPermissionMode? { mode(pending) }

  /// Shift-Tab target: the next announced mode after the current one, wrapping
  /// around and skipping full-access modes. Nothing cycles while a change is pending.
  public var nextInCycle: AgentPermissionMode? {
    guard pending == nil, !modes.isEmpty else { return nil }
    let start = modes.firstIndex { $0.id == current } ?? modes.count - 1
    for step in 1...modes.count {
      let candidate = modes[(start + step) % modes.count]
      if !candidate.fullAccess && candidate.id != current { return candidate }
    }
    return nil
  }

  public static func defaultModeKey(harness: String) -> String {
    defaultModeKeyPrefix + harness
  }
}

extension EngineClient {
  public func permissionModes(paneId: String) -> AgentPermissionModes? {
    panes.first { $0.paneId == paneId }?.agent.flatMap(AgentPermissionModes.init(agent:))
  }

  /// The one path every surface uses to change a pane's mode: a native
  /// configuration change the harness must confirm.
  public func setPermissionMode(_ paneId: String, to mode: String) {
    guard let modes = permissionModes(paneId: paneId), modes.mode(mode) != nil else { return }
    setAgentConfig(paneId, configId: modes.configId, value: .v0(mode))
  }

  public func cyclePermissionMode(_ paneId: String) {
    guard let next = permissionModes(paneId: paneId)?.nextInCycle else { return }
    setPermissionMode(paneId, to: next.id)
  }

  public func defaultPermissionMode(harness: String) -> String? {
    harnessDefaultModes[harness]
  }

  public func refreshDefaultPermissionModes() {
    perform {
      for harness in self.harnesses {
        let result = try await self.request(
          "settings.get",
          .encoded(
            SettingsGetParams(key: AgentPermissionModes.defaultModeKey(harness: harness.id)))
        ).decoded(SettingsGetResult.self)
        self.recordDefaultPermissionMode(result.value, harness: harness.id)
      }
    }
  }

  /// `nil` keeps the harness's own default for new panes.
  public func setDefaultPermissionMode(_ mode: String?, harness: String) async throws {
    let result = try await mutation(
      "settings.set",
      SettingsSetParams(
        key: AgentPermissionModes.defaultModeKey(harness: harness), value: .string(mode ?? ""),
        idempotencyKey: UUID().uuidString)
    ).decoded(SettingsSetResult.self)
    recordDefaultPermissionMode(result.value, harness: harness)
  }

  func applyDefaultPermissionModeChanged(_ event: SettingsChangedEvent) -> Bool {
    guard event.key.hasPrefix(AgentPermissionModes.defaultModeKeyPrefix) else { return false }
    let harness = String(event.key.dropFirst(AgentPermissionModes.defaultModeKeyPrefix.count))
    recordDefaultPermissionMode(event.value, harness: harness)
    return true
  }

  private func recordDefaultPermissionMode(_ value: JsonValue, harness: String) {
    harnessDefaultModes[harness] = value.string.flatMap { $0.isEmpty ? nil : $0 }
  }
}
