// Generated from contracts/protocol.json. Do not edit.
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
    /// The characters of a pairing code, and of an engine's public key, in a pairing link: 32 bytes as URL-safe base64 without padding.
    public static let pairingCodeChars = 43
    /// How long a pairing code stays valid, and the longest a device waits for the person to allow access.
    public static let pairingWindowSeconds = 600
    /// The longest name (in UTF-8 bytes) a device gives itself when it pairs. The engine shows it to the person who allows access.
    public static let deviceNameBytes = 128
    public static let promptBytes = 65536
    public static let promptQueueCount = 64
    public static let agentMessagesPerWindow = 6
    public static let agentMessageWindowSec = 60
    public static let agentMessageBytes = 60000
    public static let transcriptPageMessages = 200
    public static let transcriptPageBytes = 524288
    public static let transcriptMessageBytes = 262144
    public static let acpMessageBytes = 1048576
    public static let permissionTimeoutSec = 1800
    public static let permissionPreviewPathChars = 4096
    public static let permissionPreviewChanges = 8
    public static let permissionPreviewBytes = 65536
    public static let permissionPreviewLines = 400
    public static let permissionCommandBytes = 65536
    public static let cancelTimeoutSec = 10
    public static let probeTimeoutSec = 10
    public static let noSignalSec = 90
    public static let harnessSwitchTimeoutSec = 90
    public static let usageRefreshMinIntervalSec = 30
    public static let usageAutoRefreshIntervalSec = 300
    public static let usageStaleAfterSec = 900
    public static let usageMaxBackoffSec = 1800
    public static let usageActivityDays = 7
    public static let editorViewportLines = 2000
    public static let editorEditBytes = 262144
    /// The longest path a client links as a file reference in text (`path:line`). It bounds the clients' detector; the engine does not enforce it, and `file.open` takes the path the schema allows.
    public static let editorPathBytes = 4096
    public static let diagnosticMessageChars = 4096
    public static let editorSearchResults = 500
    public static let editorHistoryVersions = 200
    public static let editorHistoryBytes = 52428800
    public static let editorAutosaveMs = 500
    public static let projectTreeEntries = 5000
    public static let projectTreeWatches = 256
    public static let agentCommands = 256
    public static let skillBytes = 262144
    public static let skillEntries = 256
    public static let webPanesPerWorkspace = 10
    public static let webSnapshotBytes = 204800
    public static let webScreenshotBytes = 2097152
    public static let webEvaluateBytes = 65536
    public static let webConsoleEntries = 500
    public static let webNetworkEntries = 300
    public static let webWaitTimeoutMs = 60000
    public static let localServerEntries = 50
    public static let webPickedElements = 20
    public static let webOuterHtmlBytes = 2048
    public static let webFieldBytes = 4096
    public static let webTextBytes = 16384
    public static let webLabelBytes = 2048
    public static let webTitleBytes = 1024
    public static let webPartBytes = 512
    public static let webTokenBytes = 256
    public static let webExtentMax = 16384
    public static let webPickCropWidth = 640
    public static let webReadScreens = 12
    public static let webReadScreensDefault = 4
    public static let webReadTextBytes = 65536
    public static let webReadLinks = 100
    public static let webReadBytes = 524288
    public static let webAgentImageBytes = 262144
    public static let promptAttachments = 20
    public static let promptAttachmentBytes = 10485760
    public static let promptAttachmentTotalBytes = 20971520
    public static let promptAttachmentInlineBytes = 196608
    public static let promptAttachmentThumbnailBytes = 6144
    public static let webCommandWaiters = 64
    public static let portScanBytes = 1048576
    public static let writingSectionEntries = 500
    public static let writingSlides = 500
    public static let writingScenes = 1000
    public static let writingElements = 10000
    public static let writingBeatCards = 500
    public static let writingSceneLinks = 1000
    public static let writingVariantsPerLine = 12
    public static let writingVariantBytes = 16384
    public static let writingSuggestionBytes = 65536
    public static let writingSidecarBytes = 1048576
    public static let writingTitleBytes = 512
    public static let writingPathBytes = 4096
    public static let writingImageBytes = 2097152
    public static let harnessContinuationBytes = 15999
    public static let harnessContinuationMessages = 6
    public static let modelRefusals = 256
    public static let modelRefusalReasonBytes = 4096
    public static let experimentExportPanes = 64
    public static let experimentExportRows = 100000
    public static let experimentExportBytes = 67108864
    /// The most strokes one `ink.recognize` request carries.
    public static let inkStrokes = 64
    /// The most points one stroke of an `ink.recognize` request carries.
    public static let inkStrokePoints = 2000
    /// The most points all the strokes of one `ink.recognize` request carry together.
    public static let inkTotalPoints = 8000
    /// The most candidates an `ink.recognize` request may ask for and the engine answers with.
    public static let inkCandidates = 10
    /// How many candidates an `ink.recognize` request without `maxCandidates` gets.
    public static let inkCandidatesDefault = 5
    /// The most language tags one `ink.recognize` request names.
    public static let inkLanguages = 8
    /// The longest language tag (in UTF-8 bytes) of an `ink.recognize` request; the length a BCP 47 tag may have.
    public static let inkLanguageBytes = 35
    /// The largest absolute value of a stroke coordinate in an `ink.recognize` request, in the client's own units (the engine scales the writing, so the unit is the client's choice).
    public static let inkCoordinate = 1000000
    /// The longest a stroke point's time since the first point of the writing may be in an `ink.recognize` request, in milliseconds.
    public static let inkDurationMs = 3600000
    /// The longest text (in UTF-8 bytes) of one candidate the engine answers `ink.recognize` with; a longer one is cut at a character.
    public static let inkCandidateBytes = 2048
}

public enum ProtocolVersion {
    public static let current: UInt32 = 1
}

public enum FrameKind {
    public static let json: UInt8 = 1
    public static let terminal: UInt8 = 2
    public static let attachment: UInt8 = 3
}
