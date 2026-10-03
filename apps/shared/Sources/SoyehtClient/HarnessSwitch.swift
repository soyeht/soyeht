import Foundation
import SoyehtProtocol

/// The result of asking the engine to restart an agent pane on another harness.
public enum HarnessSwitchOutcome: Sendable, Equatable {
  /// The engine started the new session; the conversation was handed over.
  case switched(toHarness: String)
  /// The chosen harness cannot start, so no request was sent.
  case unavailable
  /// The engine refused or the request failed; the message is ready to show.
  case failed(String)
}

/// Shared rules for switching an agent pane's harness, used by the Mac pane
/// header and the iPhone and iPad agent pane.
public enum HarnessSwitchModel {
  public static let title = "Switch agent"
  public static let handoverExplanation =
    "This starts a new agent session. Recent messages and pending prompts are handed "
    + "over as historical data."

  /// A pane can switch while connected, with no configuration change in flight and
  /// no switch already running.
  public static func canSwitch(
    connected: Bool, agent: AgentSession?, switchState: AgentHarnessSwitchView?
  ) -> Bool {
    connected && agent?.operation == nil && !isSwitching(switchState)
  }

  public static func isSwitching(_ switchState: AgentHarnessSwitchView?) -> Bool {
    switchState?.phase == .switching
  }

  /// Every harness except the one the pane already runs, laid out the same way as
  /// the new-agent picker.
  public static func layout(
    _ harnesses: [HarnessInfo], current: String?, query: String = ""
  ) -> HarnessPickerLayout {
    HarnessPickerModel.layout(harnesses.filter { $0.id != current }, query: query)
  }

  public static func defaultTarget(
    in harnesses: [HarnessInfo], current: String?, preferredId: String?
  ) -> HarnessInfo? {
    HarnessPickerModel.defaultHarness(
      in: harnesses, excluding: current, preferredId: preferredId)
  }

  public static func actionLabel(for target: HarnessInfo?) -> String {
    guard let target else { return "Choose another agent" }
    return "Restart conversation with \(target.name)"
  }

  /// The harness's display name when the catalogue knows it, else its identifier.
  public static func displayName(of harnessId: String, in harnesses: [HarnessInfo]) -> String {
    harnesses.first { $0.id == harnessId }?.name ?? harnessId
  }

  /// A harness name without the ACP adapter's suffix ("Claude Agent" → "Claude"), the rule
  /// the engine names agent panes by.
  public static func shortName(_ name: String) -> String {
    let trimmed = name.trimmingCharacters(in: .whitespaces)
    guard trimmed.hasSuffix(adapterSuffix) else { return trimmed }
    let label = trimmed.dropLast(adapterSuffix.count).trimmingCharacters(in: .whitespaces)
    return label.isEmpty ? trimmed : label
  }

  private static let adapterSuffix = " Agent"

  /// The control that opens the switch from an agent pane: the harness the pane runs, named
  /// on a menu button ("Claude ▾"), or the one it is switching to while the switch runs.
  public static func control(
    current: String, switchState: AgentHarnessSwitchView?, requested: String? = nil,
    harnesses: [HarnessInfo]
  ) -> HarnessSwitchControl {
    let busy = requested != nil || isSwitching(switchState)
    let shown = busy ? (switchState?.toHarness ?? requested ?? current) : current
    return HarnessSwitchControl(
      title: shortName(displayName(of: shown, in: harnesses)), busy: busy)
  }

  /// Runs one switch request and turns its result or error into an outcome. An
  /// unavailable target is refused locally, without contacting the engine.
  @MainActor public static func run(
    target: HarnessInfo,
    _ request: () async throws -> PaneHarnessSwitchResult
  ) async -> HarnessSwitchOutcome {
    guard target.available else { return .unavailable }
    do {
      return .switched(toHarness: try await request().toHarness)
    } catch {
      return .failed(error.localizedDescription)
    }
  }
}

/// What the harness control of an agent pane shows, on the Mac pane header and in the
/// iPhone and iPad agent pane: the running harness named on a menu button.
public struct HarnessSwitchControl: Equatable, Sendable {
  public static let accessibilityLabel = "Switch harness"
  public static let accessibilityHint =
    "Restarts this pane on another harness and hands the conversation over"
  /// The menu indicator after the name ("Claude ▾").
  public static let symbol = "chevron.down"

  /// The running harness's short name, for example "Claude".
  public let title: String
  /// A switch is running; the title names the harness it switches to.
  public let busy: Bool

  public var help: String { "\(Self.accessibilityLabel) (running \(title))" }
}

extension EngineClient {
  public func canSwitchAgentHarness(_ paneId: String) -> Bool {
    HarnessSwitchModel.canSwitch(
      connected: connected, agent: panes.first { $0.paneId == paneId }?.agent,
      switchState: agentPanes[paneId]?.harnessSwitch)
  }

  /// Restarts the pane on `target` and hands the conversation over. `nil` model or
  /// effort leaves the choice to the harness.
  public func requestAgentHarnessSwitch(
    _ paneId: String, to target: HarnessInfo, model: String? = nil, effort: String? = nil
  ) async -> HarnessSwitchOutcome {
    await HarnessSwitchModel.run(target: target) {
      try await switchAgentHarness(paneId, harness: target.id, model: model, effort: effort)
    }
  }
}
