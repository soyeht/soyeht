// Generated from contracts/protocol.json (sha256 96e9a6c2600dc2ec7c40a94f545618ee858e5ce15ee105719d8c3c8114ec5404). Do not edit.

/// The rules every client shares, from `x-clientPolicy` in `contracts/protocol.json`.
public enum ClientPolicy {
    public static let requestTimeoutMs = 12000
    public static let requestTimeoutSeconds: Double = 12.0
    public static let greetingTimeoutMs = 5000
    public static let greetingTimeoutSeconds: Double = 5.0
    public static let reconnectDelayMs = 1000
    public static let reconnectDelaySeconds: Double = 1.0
    public static let terminalInputChunkBytes = 16384
    public static let usageBandWarningFrom: Double = 0.5
    public static let usageBandDangerAbove: Double = 0.8
    public static let contextBandWarningAbove: Double = 0.75
    public static let contextBandDangerAbove: Double = 0.9
    public static let usageRingLastKnownArcOpacity: Double = 0.45
    public static let usageRingUnavailableOpacity: Double = 0.46
    public static let usageWindowWeeklyKeyword = "week"
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
