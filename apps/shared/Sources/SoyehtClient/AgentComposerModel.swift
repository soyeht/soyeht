import SoyehtProtocol

/// One of the harness's own selectable configuration options (model or effort) as a composer
/// menu: the choices exactly as the harness advertised them, the confirmed value and the one
/// the engine is still applying.
public struct AgentConfigMenu: Equatable, Sendable {
  public struct Choice: Equatable, Sendable, Identifiable {
    public let id: String
    public let name: String
    public let detail: String?
  }

  public let configId: String
  public let title: String
  public let choices: [Choice]
  public let current: String?
  public let pending: String?

  /// `nil` unless the option is a select with at least one choice. `displayNames` maps a
  /// choice's value to the name the harness catalogue gives it (`model.list`); a choice the
  /// catalogue does not name keeps the name the option announced.
  init?(option: ConfigOption, operation: ConfigOperation?, displayNames: [String: String] = [:]) {
    guard let selection = option.selection(operation: operation) else { return nil }
    configId = option.id
    title = option.name
    choices = option.options.map {
      Choice(id: $0.value, name: displayNames[$0.value] ?? $0.name, detail: $0.description)
    }
    current = selection.current
    pending = selection.pending
  }

  /// The label on the chip: the applying choice while it is pending, else the current one.
  public var label: String {
    let shown = pending ?? current
    return choices.first { $0.id == shown }?.name ?? shown ?? title
  }
}

/// What the composer's send button does right now. While the agent runs it is a stop button
/// and Return queues the message instead of sending it.
public enum AgentComposerAction: Equatable, Sendable {
  case send(enabled: Bool)
  case stop(enabled: Bool)
}

/// The approved composer's control row (`DCzvR` in design/soyeht-desktop.pen), built from the
/// pane's ACP session: "+" · harness ▾ · model ▾ · effort ▾ · permission ▾ · context ring ·
/// mic · send. Model, effort and permission show only when the harness advertises them, and
/// the "·" separators show only between groups that are present.
public struct AgentComposerControls: Equatable, Sendable {
  public let harnessName: String
  public let model: AgentConfigMenu?
  public let effort: AgentConfigMenu?
  public let permission: AgentPermissionModes?
  /// The context ring shows for every agent session; it is empty until the harness reports
  /// its context use.
  public let showsContext: Bool
  /// Share of the context window in use, 0...1, when the harness reported it.
  public let contextFraction: Double?
  public let action: AgentComposerAction
  /// Return queues the message behind the running turn.
  public let queuesOnSubmit: Bool
  public let placeholder: String
  /// The model, effort and permission menus accept a change.
  public let configurable: Bool

  /// `catalogue` names the harness's models (`model.list`) when the client has loaded it.
  public init(
    harnessName: String, agent: AgentSession?, state: PaneState?, usage: TokenUsage?,
    hasDraft: Bool, connected: Bool, catalogue: [ModelInfo] = []
  ) {
    self.harnessName = harnessName
    let options = agent?.options ?? []
    let operation = agent?.operation
    let modelNames = Dictionary(
      catalogue.map { ($0.id, $0.name) }, uniquingKeysWith: { first, _ in first })
    model = options.first(where: \.isModel).flatMap {
      AgentConfigMenu(option: $0, operation: operation, displayNames: modelNames)
    }
    effort = options.first(where: \.isEffort).flatMap {
      AgentConfigMenu(option: $0, operation: operation)
    }
    permission = AgentPermissionModes(options: options, operation: operation)
    showsContext = agent != nil
    contextFraction = usage.flatMap(Self.fraction)
    let running = state?.isTurnRunning ?? false
    action =
      running
      ? .stop(enabled: state != .cancelling && connected)
      : .send(enabled: hasDraft && connected)
    queuesOnSubmit = running
    placeholder = running ? "Queue a message for \(harnessName)…" : "Message \(harnessName)…"
    configurable = operation == nil && connected
  }

  /// "·" between the harness and the model/effort group.
  public var showsThinkingSeparator: Bool { model != nil || effort != nil }
  /// "·" before the permission menu.
  public var showsPermissionSeparator: Bool { permission != nil }

  /// The rounded percentage the ring's label shows ("18%").
  public var contextPercentage: String? {
    contextFraction.map { "\(Int(($0 * 100).rounded()))%" }
  }

  /// Options that the composer shows as menus; a host's extra configuration row skips them.
  public static func isComposerOption(_ option: ConfigOption) -> Bool {
    option.isPermissionMode || option.isModel || option.isEffort
  }

  private static func fraction(_ usage: TokenUsage) -> Double? {
    guard let used = Double(usage.totalTokens), let size = usage.contextSize,
      let total = Double(size), total > 0
    else { return nil }
    return min(max(used / total, 0), 1)
  }
}
