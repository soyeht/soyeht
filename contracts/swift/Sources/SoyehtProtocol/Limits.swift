// Generated from contracts/protocol.json (sha256 91794bf4b1eb8414cc101b34d17bd9757b2ac69157ae51a2727c32aab9524c1f). Do not edit.
public enum ProtocolLimits {
    public static let jsonBytes = 1048576
    public static let terminalPayloadBytes = 65536
    public static let attachmentPayloadBytes = 262144
    public static let terminalHeaderBytes = 16
    public static let attachmentHeaderBytes = 16
    public static let connectionQueueBytes = 8388608
    public static let ptyRetentionBytes = 8388608
    public static let segmentBytes = 524288
    public static let paneRetentionBytes = 33554432
    public static let tailBytes = 2097152
    public static let eventCount = 10000
    public static let eventDays = 7
    public static let deviceConnections = 8
    public static let promptBytes = 65536
    public static let promptQueueCount = 64
    public static let transcriptPageMessages = 200
    public static let transcriptPageBytes = 524288
    public static let transcriptMessageBytes = 262144
    public static let acpMessageBytes = 1048576
    public static let permissionTimeoutSec = 1800
    public static let cancelTimeoutSec = 10
    public static let probeTimeoutSec = 10
    public static let noSignalSec = 90
    public static let usageRefreshMinIntervalSec = 30
    public static let usageHoverRefreshAfterSec = 60
    public static let usageOpenRefreshIntervalSec = 300
    public static let usageStaleAfterSec = 900
    public static let usageMaxBackoffSec = 1800
}
