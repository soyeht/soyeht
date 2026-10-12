import SoyehtProtocol

/// What a pane's loop panel starts from and says, shared with the desktop client through
/// `contracts/vectors/loop-panel.json`.
public enum LoopPresentation {
  public static let title = "Loop this pane"
  public static let promptHeading = "PROMPT"
  public static let whenHeading = "WHEN"
  public static let everyInterval = "Every interval"
  public static let whenIdle = "When idle"
  public static let idleExplanation = "The loop sends only when the pane is idle"
  public static let workerRole = "Worker"
  public static let leadRole = "Lead"
  public static let cancelTitle = "Cancel"
  public static let applyTitle = "Apply"
  public static let rules = [
    "Never sends over your draft.", "Busy ticks are skipped and recorded.",
    "Survives an engine restart.",
  ]

  public static let defaultPrompt = "Pick the next task and complete it."
  public static let defaultInterval: UInt32 = 60
  public static let defaultMaxRounds: UInt32 = 10
  public static let defaultStopWhen = LoopStopWhen.three_empty

  public static let intervalChoices: [UInt32] = [5, 10, 30, 60, 300, 600, 1_800, 3_600]
  public static let roundChoices: [UInt32] = [0, 1, 5, 10, 25, 50, 100]
  public static let stopChoices: [LoopStopWhen] = [.three_empty, .round_limit, .explicit_off]

  public static func intervalLabel(_ seconds: UInt32) -> String {
    if seconds < 60 { return "\(seconds) sec" }
    return "\(seconds / 60) min"
  }

  public static func roundLabel(_ rounds: UInt32) -> String {
    rounds == 0 ? "no limit" : "\(rounds) rounds"
  }

  /// The short name on the option's pill.
  public static func stopLabel(_ stopWhen: LoopStopWhen) -> String {
    switch stopWhen {
    case .three_empty: "three empty"
    case .round_limit: "round limit"
    case .explicit_off: "until off"
    }
  }

  /// The name of the choice in the option's menu.
  public static func stopChoiceTitle(_ stopWhen: LoopStopWhen) -> String {
    switch stopWhen {
    case .three_empty: "Three empty results"
    case .round_limit: "Round limit"
    case .explicit_off: "Until turned off"
    }
  }

  /// The line in the footer: when the next tick is due and how many ticks ran today.
  public static func status(enabled: Bool, paneName: String, rounds: UInt32) -> String {
    enabled ? "next: when \(paneName) is idle · \(rounds) today" : "off · \(rounds) today"
  }

  /// The values the panel opens with: the pane's loop, else the defaults.
  public static func initial(_ config: LoopConfig?) -> (
    prompt: String, interval: UInt32, maxRounds: UInt32, stopWhen: LoopStopWhen, enabled: Bool
  ) {
    guard let config else {
      return (defaultPrompt, defaultInterval, defaultMaxRounds, defaultStopWhen, false)
    }
    return (config.prompt, config.interval, config.maxRounds, config.stopWhen, config.enabled)
  }

  /// Applying a loop that is on needs a prompt to send.
  public static func canApply(enabled: Bool, prompt: String) -> Bool {
    !(enabled && prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
  }
}
