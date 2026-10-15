// Generated from contracts/protocol.json (sha256 5c9246216c7089634574b1e4ee21720d759b8abed6a21bfac81bffec76c08a3e). Do not edit.

/// The rules every client shares, from `x-clientPolicy` in `contracts/protocol.json`.
public enum ClientPolicy {
    public static let requestTimeoutMs = 12000
    public static let requestTimeoutSeconds: Double = 12.0
    public static let greetingTimeoutMs = 5000
    public static let greetingTimeoutSeconds: Double = 5.0
    public static let reconnectDelayMs = 1000
    public static let reconnectDelaySeconds: Double = 1.0
    public static let reconnectCeilingMs = 30000
    public static let reconnectCeilingSeconds: Double = 30.0
    public static let draftBeatMs = 8000
    public static let draftBeatSeconds: Double = 8.0
    public static let terminalInputChunkBytes = 16384
    public static let pageSizeQueue: UInt32 = 64
    public static let pageSizeHarnesses: UInt32 = 64
    public static let pageSizeModels: UInt32 = 64
    public static let pageSizeTranscript: UInt32 = 100
    public static let pageSizeTasks: UInt32 = 100
    public static let harnessSwitchTitle = "Switch agent"
    public static let harnessSwitchHandover = "This starts a new agent session. Recent messages and pending prompts are handed over as historical data."
    public static let taskMoveReason = "Moved on the workspace board"
    public static let usageBandWarningFrom: Double = 0.5
    public static let usageBandDangerAbove: Double = 0.8
    public static let contextBandWarningAbove: Double = 0.75
    public static let contextBandDangerAbove: Double = 0.9
    public static let usageRingLastKnownArcOpacity: Double = 0.45
    public static let usageRingUnavailableOpacity: Double = 0.46
    public static let usageWindowWeeklyKeyword = "week"

    /// Image types that claude-agent-acp and codex-acp forward as model input and that both
    /// providers accept; any other image is linked.
    public static let inlineImageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"]
}

/// The palette role a pane state's dot is colored from; each client resolves the role.
public enum StateDot: String, Sendable, CaseIterable {
    case success, accent, warning, muted, secondary
}

extension PaneState {
    /// Whether an agent in this state is in the middle of a turn: it can be stopped, what
    /// is typed is queued, and the header counts the time.
    public var isTurnRunning: Bool {
        switch self {
        case .working, .waiting_for_you, .cancelling, .no_signal: true
        default: false
        }
    }

    /// The role of the state's dot.
    public var dot: StateDot {
        switch self {
        case .starting: .secondary
        case .working: .success
        case .idle: .success
        case .waiting_for_you: .accent
        case .cancelling: .secondary
        case .no_signal: .warning
        case .recovering: .secondary
        case .dead: .muted
        case .unknown: .muted
        }
    }
}

extension UsageProvider {
    /// The name the clients show for the provider.
    public var displayName: String {
        switch self {
            case .claude: "Claude"
            case .codex: "Codex"
            case .cursor: "Cursor"
        }
    }
}

extension ConfigOption {
    // A selector has a role when its category (its id when it has none) names it.
    /// Whether this option is the agent's model selector.
    public var isModel: Bool { (category ?? id) == "model" }

    /// Whether this option is the agent's effort selector.
    public var isEffort: Bool { (category ?? id) == "thought_level" || id == "effort" }

    /// Whether this option is the agent's permission selector.
    public var isPermissionMode: Bool { (category ?? id) == "mode" || id == "mode" }
}
