import Foundation
import SoyehtProtocol

extension ConfigOption {
  /// A selectable option's chosen value, and the one `operation` is still applying to it.
  func selection(operation: ConfigOperation?) -> (current: String?, pending: String?)? {
    guard type == .select, !options.isEmpty else { return nil }
    var current: String?
    if case .v0(let value) = currentValue { current = value }
    var pending: String?
    if let operation, operation.configId == id, case .v0(let value) = operation.value {
      pending = value
    }
    return (current, pending)
  }
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

  public let configId: String
  public let modes: [AgentPermissionMode]
  public let current: String?
  public let pending: String?

  public init?(options: [ConfigOption], operation: ConfigOperation? = nil) {
    guard let option = options.first(where: \.isPermissionMode),
      let selection = option.selection(operation: operation)
    else { return nil }
    configId = option.id
    modes = option.options.map {
      AgentPermissionMode(
        id: $0.value, name: $0.name, detail: $0.description,
        fullAccess: $0.kind == Self.fullAccessKind)
    }
    current = selection.current
    pending = selection.pending
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
    harnessDefaultModes[harness] ?? nil
  }

  /// Asks the engine for the default mode of each harness it has not been asked about. Opening
  /// Preferences again asks nothing: the answers are kept, and `settings.changed` keeps them right.
  @discardableResult
  public func refreshDefaultPermissionModes() -> Task<Void, Never> {
    performBackground(.defaultPermissionModes) {
      for harness in self.harnesses where !self.harnessDefaultModes.keys.contains(harness.id) {
        let result = try await self.request(
          .settingsGet,
          .encoded(
            SettingsGetParams(key: Setting.harnessDefaultModeKey(harness.id)))
        ).decoded(SettingsGetResult.self)
        self.recordDefaultPermissionMode(result.value, harness: harness.id)
      }
    }
  }

  /// `nil` keeps the harness's own default for new panes.
  public func setDefaultPermissionMode(_ mode: String?, harness: String) async throws {
    let result = try await mutation(
      .settingsSet,
      SettingsSetParams(
        key: Setting.harnessDefaultModeKey(harness), value: .string(mode ?? ""),
        idempotencyKey: UUID().uuidString)
    ).decoded(SettingsSetResult.self)
    recordDefaultPermissionMode(result.value, harness: harness)
  }

  func applyDefaultPermissionModeChanged(_ event: SettingsChangedEvent) -> Bool {
    guard let harness = Setting.harnessDefaultModeParameter(of: event.key) else { return false }
    recordDefaultPermissionMode(event.value, harness: harness)
    return true
  }

  private func recordDefaultPermissionMode(_ value: JsonValue, harness: String) {
    harnessDefaultModes.updateValue(
      value.string.flatMap { $0.isEmpty ? nil : $0 }, forKey: harness)
  }
}
