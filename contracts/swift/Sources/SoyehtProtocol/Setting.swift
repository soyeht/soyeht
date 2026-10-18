// Generated from contracts/protocol.json. Do not edit.

/// The settings the engine stores, from `x-settings` in `contracts/protocol.json`.
public enum Setting {
    public static let terminalDefaultCols = "terminal.defaultCols"
    public static let terminalDefaultRows = "terminal.defaultRows"
    public static let recordDefault = "record.default"
    public static let harnessPreferredId = "harness.preferredId"
    public static let harnessDefaultModePrefix = "harness.defaultMode."

    /// Every setting that stands alone.
    public static let keys = [terminalDefaultCols, terminalDefaultRows, recordDefault, harnessPreferredId]

    /// The key of the `harness.defaultMode.<harnessId>` setting for `harnessId`.
    public static func harnessDefaultModeKey(_ harnessId: String) -> String { harnessDefaultModePrefix + harnessId }

    /// The `harnessId` of a `harness.defaultMode.<harnessId>` key, when `key` is one.
    public static func harnessDefaultModeParameter(of key: String) -> String? {
        guard key.hasPrefix(harnessDefaultModePrefix) else { return nil }
        let harnessId = String(key.dropFirst(harnessDefaultModePrefix.count))
        return (1...128).contains(harnessId.utf8.count) ? harnessId : nil
    }

    /// The value a setting has until someone changes it; `nil` for a key that is not a setting.
    public static func defaultValue(for key: String) -> JsonValue? {
        switch key {
        case terminalDefaultCols: .integer(80)
        case terminalDefaultRows: .integer(24)
        case recordDefault: .bool(true)
        case harnessPreferredId: .string("")
        case _ where harnessDefaultModeParameter(of: key) != nil: .string("")
        default: nil
        }
    }

    /// Whether `value` may be stored under `key`.
    public static func accepts(_ value: JsonValue, for key: String) -> Bool {
        switch key {
        case terminalDefaultCols:
            guard case .integer(let number) = value else { return false }
            return (1...1000).contains(number)
        case terminalDefaultRows:
            guard case .integer(let number) = value else { return false }
            return (1...1000).contains(number)
        case recordDefault:
            if case .bool = value { return true }
            return false
        case harnessPreferredId:
            guard case .string(let text) = value else { return false }
            return text.utf8.count <= 128 && !text.contains("\0")
        case _ where harnessDefaultModeParameter(of: key) != nil:
            guard case .string(let text) = value else { return false }
            return text.utf8.count <= 128 && !text.contains("\0")
        default: return false
        }
    }
}
