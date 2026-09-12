// Generated from contracts/protocol.json (sha256 742ff26d144d2473e0f726b9aa6cb686deb01ff88adc3a4a3b4f2ab81a946e67). Do not edit.
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
}
