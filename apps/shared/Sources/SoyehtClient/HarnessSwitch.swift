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
  ///
  /// The title is the engine's display name from the harness catalogue (`harness.list`
  /// already applies the engine's naming rule). Before the catalogue loads, the running
  /// harness falls back to the pane's display name, and any other harness to a name read
  /// from its identifier. A raw launch identifier is never shown.
  public static func control(
    current: String, switchState: AgentHarnessSwitchView?, requested: String? = nil,
    harnesses: [HarnessInfo], paneName: String?
  ) -> HarnessSwitchControl {
    let busy = requested != nil || isSwitching(switchState)
    let shown = busy ? (switchState?.toHarness ?? requested ?? current) : current
    let title: String
    if let entry = harnesses.first(where: { $0.id == shown }) {
      title = shortName(entry.name)
    } else if shown == current,
      let paneName = paneName?.trimmingCharacters(in: .whitespaces), !paneName.isEmpty
    {
      title = paneName
    } else {
      title = readableName(ofHarnessId: shown)
    }
    return HarnessSwitchControl(title: title, busy: busy)
  }

  /// A last-resort name from a launch identifier: "registry.claude-acp" → "Claude".
  static func readableName(ofHarnessId harnessId: String) -> String {
    var name = Substring(harnessId)
    for prefix in ["registry.", "local.", "custom."] where name.hasPrefix(prefix) {
      name = name.dropFirst(prefix.count)
    }
    if name.hasSuffix("-acp") { name = name.dropLast("-acp".count) }
    let words = name.split(whereSeparator: { $0 == "-" || $0 == "_" || $0 == "." })
    guard !words.isEmpty else { return "Agent" }
    return words.map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
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

/// What the harness chip of an agent composer shows, on the Mac, iPhone and iPad: the running
/// harness named on a menu button.
public struct HarnessSwitchControl: Equatable, Sendable {
  public static let accessibilityLabel = "Switch harness"
  public static let accessibilityHint =
    "Restarts this pane on another harness and hands the conversation over"

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
