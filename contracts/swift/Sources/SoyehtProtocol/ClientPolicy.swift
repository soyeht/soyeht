// Generated from contracts/protocol.json. Do not edit.

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
    public static let agentPartialFlushMs = 16
    public static let agentPartialFlushSeconds: Double = 0.016
    public static let terminalInputChunkBytes = 16384
    public static let terminalScrollbackLines = 10000
    public static let capsWaitingMessages = 1000
    public static let capsFileLinks = 128
    public static let capsWebLinks = 128
    public static let capsModelPages = 32
    public static let capsTickerHistory = 3
    public static let pageSizeQueue: UInt32 = 64
    public static let pageSizeHarnesses: UInt32 = 64
    public static let pageSizeModels: UInt32 = 64
    public static let pageSizeTranscript: UInt32 = 100
    public static let pageSizeTasks: UInt32 = 100
    public static let pageSizeWritingSection: UInt32 = 100
    public static let harnessSwitchTitle = "Switch agent"
    public static let harnessSwitchHandover = "This starts a new agent session. Recent messages and pending prompts are handed over as historical data."
    public static let taskMoveReason = "Moved on the workspace board"
    public static let editorNoFile = "The engine has no file for this editor tab."
    public static let editorFileNotText = "Binary file, can't be shown."
    public static let editorFileTooLarge = "This file is too large to open in the editor."
    public static let editorSaveMissing = "Couldn't save: this file was deleted from the disk. Save again to create it."
    public static let editorSaveDenied = "Couldn't save: permission denied. Fix the permissions, then save again."
    public static let editorSaveFull = "Couldn't save: the disk is full. Free some space, then save again."
    public static let editorSaveOther = "Couldn't save this file. The engine keeps trying; save again to retry now."
    public static let editorSaveAgain = "Save again"
    public static let usageBandWarningFrom: Double = 0.5
    public static let usageBandDangerAbove: Double = 0.8
    public static let contextBandWarningAbove: Double = 0.75
    public static let contextBandDangerAbove: Double = 0.9
    public static let usageRingLastKnownArcOpacity: Double = 0.45
    public static let usageRingUnavailableOpacity: Double = 0.46
    public static let usageWindowWeeklyKeyword = "week"
    public static let editorPrefetchLines = 160
    public static let editorPageLeadDivisor = 4
    public static let editorTabWidth = 4
    public static let editorTabHistory = 64
    public static let editorSaveAttempts = 30
    public static let editorSaveRetryMs = 100
    public static let editorSaveRetrySeconds: Double = 0.1
    public static let editorIndentUnit = "    "
    public static let editorIndentOpeners = "{[("

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

extension ClientPolicy {
    /// The character an editor types after `opening` to close the pair, when it closes one
    /// (brackets and quotes).
    public static func editorClosing(_ opening: String) -> String? {
        switch opening {
        case "(": ")"
        case "[": "]"
        case "{": "}"
        case "\"": "\""
        case "'": "'"
        default: nil
        }
    }

    /// Whether `typed` is a character that closes a pair (a closing bracket or a quote), so
    /// typing it over the same character already there steps over it.
    public static func editorClosesPair(_ typed: String) -> Bool {
        switch typed {
        case ")", "]", "}", "\"", "'": true
        default: false
        }
    }
}

/// The palette role a syntax highlight is colored from; each client resolves the role.
public enum HighlightRole: String, Sendable, CaseIterable {
    case accent, syntaxType, warning, muted, success, ink
}

extension HighlightKind {
    /// The role of the highlight's color.
    public var role: HighlightRole {
        switch self {
        case .keyword: .accent
        case .string: .success
        case .comment: .muted
        case .number: .warning
        case .type: .syntaxType
        case .function: .accent
        case .property: .ink
        case .operator: .accent
        case .punctuation: .ink
        case .constant: .warning
        case .variable: .ink
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
