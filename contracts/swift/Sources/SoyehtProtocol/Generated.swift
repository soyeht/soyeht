// Generated from contracts/protocol.json (sha256 6a3d8835f9436a138b0fd10911e961d4d3f0fa8765f7ee08b980a55379f0eacd). Do not edit.
import Foundation



public typealias Id = String

public typealias Decimal = String

public typealias ByteString = String

public enum Role: String, Codable, Sendable {
    case `human_local` = "human_local"
    case `human_device` = "human_device"
    case `agent` = "agent"
    case `chief` = "chief"
}

public enum AgentPaneRole: String, Codable, Sendable {
    case `worker` = "worker"
    case `chief` = "chief"
}

public enum PaneMode: String, Codable, Sendable {
    case `terminal` = "terminal"
    case `agent` = "agent"
    case `editor` = "editor"
    case `web` = "web"
    case `writing` = "writing"
}

public enum PaneState: String, Codable, Sendable {
    case `starting` = "starting"
    case `working` = "working"
    case `idle` = "idle"
    case `waiting_for_you` = "waiting_for_you"
    case `cancelling` = "cancelling"
    case `no_signal` = "no_signal"
    case `recovering` = "recovering"
    case `dead` = "dead"
    case `unknown` = "unknown"
}

public enum StateSource: String, Codable, Sendable {
    case `protocol` = "protocol"
    case `hook` = "hook"
    case `osc` = "osc"
    case `screen` = "screen"
    case `none` = "none"
}

public enum SnapshotSection: String, Codable, Sendable {
    case `workspaces` = "workspaces"
    case `panes` = "panes"
    case `tasks` = "tasks"
    case `requests` = "requests"
    case `attention` = "attention"
    case `loops` = "loops"
    case `devices` = "devices"
    case `capabilities` = "capabilities"
    case `streams` = "streams"
    case `files` = "files"
    case `writing` = "writing"
}

public enum ErrorKind: String, Codable, Sendable {
    case `not_found` = "not_found"
    case `invalid_params` = "invalid_params"
    case `unauthorized` = "unauthorized"
    case `forbidden_role` = "forbidden_role"
    case `pane_busy` = "pane_busy"
    case `task_has_owner` = "task_has_owner"
    case `transition_refused` = "transition_refused"
    case `stale_version` = "stale_version"
    case `idempotency_conflict` = "idempotency_conflict"
    case `harness_unavailable` = "harness_unavailable"
    case `needs_login` = "needs_login"
    case `cursor_too_old` = "cursor_too_old"
    case `resync_required` = "resync_required"
    case `slow_consumer` = "slow_consumer"
    case `protocol_version_unsupported` = "protocol_version_unsupported"
    case `disk_full` = "disk_full"
    case `ptyd_unavailable` = "ptyd_unavailable"
    case `stale_engine_generation` = "stale_engine_generation"
    case `internal_error` = "internal_error"
    case `snapshot_too_large` = "snapshot_too_large"
    case `engine_identity_changed` = "engine_identity_changed"
    case `stale_attempt` = "stale_attempt"
    case `dependency_cycle` = "dependency_cycle"
    case `dependency_open` = "dependency_open"
    case `rate_limited` = "rate_limited"
    case `stale_reference` = "stale_reference"
    case `human_took_over` = "human_took_over"
    case `web_host_unavailable` = "web_host_unavailable"
    case `navigation_refused` = "navigation_refused"
    case `writing_kind_refused` = "writing_kind_refused"
    case `writing_limit_exceeded` = "writing_limit_exceeded"
    case `writing_sidecar_conflict` = "writing_sidecar_conflict"
    case `writing_line_not_found` = "writing_line_not_found"
    case `writing_scene_not_found` = "writing_scene_not_found"
    case `writing_section_refused` = "writing_section_refused"
    case `harness_switch_failed` = "harness_switch_failed"
}

public struct RpcError: Codable, Sendable {
    public var `code`: Int64
    public var `message`: String
    public var `data`: ErrorData
    public init(`code`: Int64, `message`: String, `data`: ErrorData) {
        self.`code` = `code`
        self.`message` = `message`
        self.`data` = `data`
    }
}

public struct ErrorData: Codable, Sendable {
    public var `kind`: ErrorKind
    public var `detail`: String?
    public var `retryAfterSec`: UInt32?
    public init(`kind`: ErrorKind, `detail`: String? = nil, `retryAfterSec`: UInt32? = nil) {
        self.`kind` = `kind`
        self.`detail` = `detail`
        self.`retryAfterSec` = `retryAfterSec`
    }
}

public struct Actor: Codable, Sendable {
    public var `role`: Role
    public var `client`: String
    public var `deviceId`: Id?
    public var `paneId`: Id?
    public init(`role`: Role, `client`: String, `deviceId`: Id? = nil, `paneId`: Id? = nil) {
        self.`role` = `role`
        self.`client` = `client`
        self.`deviceId` = `deviceId`
        self.`paneId` = `paneId`
    }
}

public struct Workspace: Codable, Sendable {
    public var `workspaceId`: Id
    public var `name`: String
    public var `path`: String
    public var `layout`: JsonValue
    public init(`workspaceId`: Id, `name`: String, `path`: String, `layout`: JsonValue) {
        self.`workspaceId` = `workspaceId`
        self.`name` = `name`
        self.`path` = `path`
        self.`layout` = `layout`
    }
}

public struct Pane: Codable, Sendable {
    public var `paneId`: Id
    public var `workspaceId`: Id
    public var `name`: String
    public var `cwd`: String
    public var `mode`: PaneMode
    public var `state`: PaneState
    public var `source`: StateSource
    public var `confidence`: Double
    public var `phrase`: String
    public var `generation`: UInt32
    public var `cols`: UInt32
    public var `rows`: UInt32
    public var `record`: Bool
    public var `experimentCapture`: PaneExperimentCapture?
    public var `pid`: UInt32?
    public var `streamId`: Id?
    public var `agent`: AgentSession?
    public var `url`: String?
    public var `title`: String?
    public var `viewport`: WebViewport?
    public var `incognito`: Bool?
    public var `webSession`: WebSessionState?
    public var `writingKind`: WritingKind?
    public var `writingSection`: WritingSection?
    public init(`paneId`: Id, `workspaceId`: Id, `name`: String, `cwd`: String, `mode`: PaneMode, `state`: PaneState, `source`: StateSource, `confidence`: Double, `phrase`: String, `generation`: UInt32, `cols`: UInt32, `rows`: UInt32, `record`: Bool, `experimentCapture`: PaneExperimentCapture? = nil, `pid`: UInt32? = nil, `streamId`: Id? = nil, `agent`: AgentSession? = nil, `url`: String? = nil, `title`: String? = nil, `viewport`: WebViewport? = nil, `incognito`: Bool? = nil, `webSession`: WebSessionState? = nil, `writingKind`: WritingKind? = nil, `writingSection`: WritingSection? = nil) {
        self.`paneId` = `paneId`
        self.`workspaceId` = `workspaceId`
        self.`name` = `name`
        self.`cwd` = `cwd`
        self.`mode` = `mode`
        self.`state` = `state`
        self.`source` = `source`
        self.`confidence` = `confidence`
        self.`phrase` = `phrase`
        self.`generation` = `generation`
        self.`cols` = `cols`
        self.`rows` = `rows`
        self.`record` = `record`
        self.`experimentCapture` = `experimentCapture`
        self.`pid` = `pid`
        self.`streamId` = `streamId`
        self.`agent` = `agent`
        self.`url` = `url`
        self.`title` = `title`
        self.`viewport` = `viewport`
        self.`incognito` = `incognito`
        self.`webSession` = `webSession`
        self.`writingKind` = `writingKind`
        self.`writingSection` = `writingSection`
    }
}

public struct Device: Codable, Sendable {
    public var `deviceId`: Id
    public var `name`: String
    public var `fingerprint`: String
    public var `revoked`: Bool
    public init(`deviceId`: Id, `name`: String, `fingerprint`: String, `revoked`: Bool) {
        self.`deviceId` = `deviceId`
        self.`name` = `name`
        self.`fingerprint` = `fingerprint`
        self.`revoked` = `revoked`
    }
}

public struct StreamInfo: Codable, Sendable {
    public var `streamId`: Id
    public var `paneId`: Id
    public var `generation`: UInt32
    public var `firstRetainedOffset`: Decimal
    public var `nextOffset`: Decimal
    public var `binaryPaneRef`: UInt32
    public init(`streamId`: Id, `paneId`: Id, `generation`: UInt32, `firstRetainedOffset`: Decimal, `nextOffset`: Decimal, `binaryPaneRef`: UInt32) {
        self.`streamId` = `streamId`
        self.`paneId` = `paneId`
        self.`generation` = `generation`
        self.`firstRetainedOffset` = `firstRetainedOffset`
        self.`nextOffset` = `nextOffset`
        self.`binaryPaneRef` = `binaryPaneRef`
    }
}

public struct PtydStatus: Codable, Sendable {
    public var `connected`: Bool
    public var `version`: UInt32
    public var `generation`: UInt32
    public init(`connected`: Bool, `version`: UInt32, `generation`: UInt32) {
        self.`connected` = `connected`
        self.`version` = `version`
        self.`generation` = `generation`
    }
}

public struct DbStatus: Codable, Sendable {
    public var `id`: Id
    public var `schemaVersion`: UInt32
    public init(`id`: Id, `schemaVersion`: UInt32) {
        self.`id` = `id`
        self.`schemaVersion` = `schemaVersion`
    }
}

public struct StreamCursor: Codable, Sendable {
    public var `streamId`: Id
    public var `offset`: Decimal
    public init(`streamId`: Id, `offset`: Decimal) {
        self.`streamId` = `streamId`
        self.`offset` = `offset`
    }
}

public enum ReplayFrom: Codable, Sendable {
    case v0(TailSelector)
    case v1(StreamCursor)
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(TailSelector.self) { self = .v0(value); return }
        if let value = try? container.decode(StreamCursor.self) { self = .v1(value); return }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "No matching ReplayFrom variant")
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .v0(let value): try container.encode(value)
        case .v1(let value): try container.encode(value)
        }
    }
}

public struct Welcome: Codable, Sendable {
    public var `engineVersion`: String
    public var `protocolVersion`: UInt32
    public var `connectionId`: Id
    public var `role`: Role
    public var `capabilities`: [String]
    public var `dbId`: Id
    public init(`engineVersion`: String, `protocolVersion`: UInt32, `connectionId`: Id, `role`: Role, `capabilities`: [String], `dbId`: Id) {
        self.`engineVersion` = `engineVersion`
        self.`protocolVersion` = `protocolVersion`
        self.`connectionId` = `connectionId`
        self.`role` = `role`
        self.`capabilities` = `capabilities`
        self.`dbId` = `dbId`
    }
}

public struct Empty: Codable, Sendable {

    public init() {

    }
}

public struct HelloParams: Codable, Sendable {
    public var `client`: String
    public var `clientVersion`: String
    public var `protocolVersion`: UInt32
    public var `deviceToken`: String?
    public var `proof`: ByteString?
    public var `binding`: String?
    public var `capabilities`: [ClientCapability]?
    public init(`client`: String, `clientVersion`: String, `protocolVersion`: UInt32, `deviceToken`: String? = nil, `proof`: ByteString? = nil, `binding`: String? = nil, `capabilities`: [ClientCapability]? = nil) {
        self.`client` = `client`
        self.`clientVersion` = `clientVersion`
        self.`protocolVersion` = `protocolVersion`
        self.`deviceToken` = `deviceToken`
        self.`proof` = `proof`
        self.`binding` = `binding`
        self.`capabilities` = `capabilities`
    }
}

public struct StateSnapshotParams: Codable, Sendable {

    public init() {

    }
}

public struct StateSnapshotResult: Codable, Sendable {
    public var `snapshotId`: Id
    public var `cursor`: Decimal
    public var `parts`: UInt32
    public var `focus`: ClientFocus?
    public init(`snapshotId`: Id, `cursor`: Decimal, `parts`: UInt32, `focus`: ClientFocus? = nil) {
        self.`snapshotId` = `snapshotId`
        self.`cursor` = `cursor`
        self.`parts` = `parts`
        self.`focus` = `focus`
    }
}

public struct EventsSubscribeParams: Codable, Sendable {
    public var `since`: Decimal
    public init(`since`: Decimal) {
        self.`since` = `since`
    }
}

public struct EventsSubscribeResult: Codable, Sendable {
    public var `subscriptionId`: Id
    public var `through`: Decimal
    public init(`subscriptionId`: Id, `through`: Decimal) {
        self.`subscriptionId` = `subscriptionId`
        self.`through` = `through`
    }
}

public struct WorkspaceListParams: Codable, Sendable {

    public init() {

    }
}

public struct WorkspaceListResult: Codable, Sendable {
    public var `items`: [Workspace]
    public init(`items`: [Workspace]) {
        self.`items` = `items`
    }
}

public struct WorkspaceCreateParams: Codable, Sendable {
    public var `name`: String
    public var `path`: String
    public var `idempotencyKey`: Id
    public init(`name`: String, `path`: String, `idempotencyKey`: Id) {
        self.`name` = `name`
        self.`path` = `path`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WorkspaceCreateResult: Codable, Sendable {
    public var `workspace`: Workspace
    public init(`workspace`: Workspace) {
        self.`workspace` = `workspace`
    }
}

public struct WorkspaceRenameParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `name`: String
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `name`: String, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`name` = `name`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WorkspaceRenameResult: Codable, Sendable {
    public var `workspace`: Workspace
    public init(`workspace`: Workspace) {
        self.`workspace` = `workspace`
    }
}

public struct WorkspaceCloseParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WorkspaceCloseResult: Codable, Sendable {
    public var `workspaceId`: Id
    public init(`workspaceId`: Id) {
        self.`workspaceId` = `workspaceId`
    }
}

public struct WorkspaceFocusParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WorkspaceFocusResult: Codable, Sendable {
    public var `workspaceId`: Id
    public init(`workspaceId`: Id) {
        self.`workspaceId` = `workspaceId`
    }
}

public struct PaneCreateParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `mode`: PaneMode
    public var `cwd`: String?
    public var `name`: String?
    public var `cols`: UInt32?
    public var `rows`: UInt32?
    public var `idempotencyKey`: Id
    public var `harness`: Id?
    public var `model`: String?
    public var `effort`: String?
    public var `permission`: PermissionMode?
    public var `url`: String?
    public var `incognito`: Bool?
    public var `path`: String?
    public var `writingKind`: WritingKind?
    public var `writingSection`: WritingSection?
    public init(`workspaceId`: Id, `mode`: PaneMode, `cwd`: String? = nil, `name`: String? = nil, `cols`: UInt32? = nil, `rows`: UInt32? = nil, `idempotencyKey`: Id, `harness`: Id? = nil, `model`: String? = nil, `effort`: String? = nil, `permission`: PermissionMode? = nil, `url`: String? = nil, `incognito`: Bool? = nil, `path`: String? = nil, `writingKind`: WritingKind? = nil, `writingSection`: WritingSection? = nil) {
        self.`workspaceId` = `workspaceId`
        self.`mode` = `mode`
        self.`cwd` = `cwd`
        self.`name` = `name`
        self.`cols` = `cols`
        self.`rows` = `rows`
        self.`idempotencyKey` = `idempotencyKey`
        self.`harness` = `harness`
        self.`model` = `model`
        self.`effort` = `effort`
        self.`permission` = `permission`
        self.`url` = `url`
        self.`incognito` = `incognito`
        self.`path` = `path`
        self.`writingKind` = `writingKind`
        self.`writingSection` = `writingSection`
    }
}

public struct PaneCreateResult: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct PaneCloseParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneCloseResult: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct PaneFocusParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneFocusResult: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct PaneRenameParams: Codable, Sendable {
    public var `paneId`: Id
    public var `name`: String
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `name`: String, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`name` = `name`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneRenameResult: Codable, Sendable {
    public var `pane`: Pane
    public init(`pane`: Pane) {
        self.`pane` = `pane`
    }
}

public struct PaneResizeParams: Codable, Sendable {
    public var `paneId`: Id
    public var `cols`: UInt32
    public var `rows`: UInt32
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `cols`: UInt32, `rows`: UInt32, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`cols` = `cols`
        self.`rows` = `rows`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneResizeResult: Codable, Sendable {
    public var `cols`: UInt32
    public var `rows`: UInt32
    public init(`cols`: UInt32, `rows`: UInt32) {
        self.`cols` = `cols`
        self.`rows` = `rows`
    }
}

public struct PaneInputParams: Codable, Sendable {
    public var `paneId`: Id
    public var `bytes`: ByteString
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `bytes`: ByteString, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`bytes` = `bytes`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneInputResult: Codable, Sendable {
    public var `acceptedBytes`: UInt32
    public init(`acceptedBytes`: UInt32) {
        self.`acceptedBytes` = `acceptedBytes`
    }
}

public struct PaneStreamSubscribeParams: Codable, Sendable {
    public var `paneId`: Id
    public var `from`: ReplayFrom
    public init(`paneId`: Id, `from`: ReplayFrom) {
        self.`paneId` = `paneId`
        self.`from` = `from`
    }
}

public struct PaneStreamUnsubscribeParams: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct PaneRecordSetParams: Codable, Sendable {
    public var `paneId`: Id
    public var `on`: Bool
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `on`: Bool, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`on` = `on`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneRecordSetResult: Codable, Sendable {
    public var `on`: Bool
    public init(`on`: Bool) {
        self.`on` = `on`
    }
}

public struct PaneExperimentCapture: Codable, Sendable {
    public var `on`: Bool
    public var `failure`: String?
    public init(`on`: Bool, `failure`: String? = nil) {
        self.`on` = `on`
        self.`failure` = `failure`
    }
}

public struct PaneExperimentSetParams: Codable, Sendable {
    public var `paneId`: Id
    public var `on`: Bool
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `on`: Bool, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`on` = `on`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneExperimentSetResult: Codable, Sendable {
    public var `capture`: PaneExperimentCapture
    public init(`capture`: PaneExperimentCapture) {
        self.`capture` = `capture`
    }
}

public struct PaneExperimentDeleteParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneExperimentDeleteResult: Codable, Sendable {
    public var `files`: UInt32
    public var `bytes`: Decimal
    public var `nothingRemained`: Bool
    public init(`files`: UInt32, `bytes`: Decimal, `nothingRemained`: Bool) {
        self.`files` = `files`
        self.`bytes` = `bytes`
        self.`nothingRemained` = `nothingRemained`
    }
}

public struct DevicePairStartParams: Codable, Sendable {
    public var `idempotencyKey`: Id
    public init(`idempotencyKey`: Id) {
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct DevicePairStartResult: Codable, Sendable {
    public var `code`: String
    public var `expiresAt`: String
    public var `uri`: String
    public init(`code`: String, `expiresAt`: String, `uri`: String) {
        self.`code` = `code`
        self.`expiresAt` = `expiresAt`
        self.`uri` = `uri`
    }
}

public struct DevicePairDecideParams: Codable, Sendable {
    public var `requestId`: Id
    public var `approve`: Bool
    public var `idempotencyKey`: Id
    public init(`requestId`: Id, `approve`: Bool, `idempotencyKey`: Id) {
        self.`requestId` = `requestId`
        self.`approve` = `approve`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct DevicePairDecideResult: Codable, Sendable {
    public var `requestId`: Id
    public var `approved`: Bool
    public init(`requestId`: Id, `approved`: Bool) {
        self.`requestId` = `requestId`
        self.`approved` = `approved`
    }
}

public struct DeviceListParams: Codable, Sendable {

    public init() {

    }
}

public struct DeviceListResult: Codable, Sendable {
    public var `items`: [Device]
    public init(`items`: [Device]) {
        self.`items` = `items`
    }
}

public struct DeviceRevokeParams: Codable, Sendable {
    public var `deviceId`: Id
    public var `idempotencyKey`: Id
    public init(`deviceId`: Id, `idempotencyKey`: Id) {
        self.`deviceId` = `deviceId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct DeviceRevokeResult: Codable, Sendable {
    public var `deviceId`: Id
    public init(`deviceId`: Id) {
        self.`deviceId` = `deviceId`
    }
}

public struct PairBeginParams: Codable, Sendable {
    public var `code`: String
    public var `deviceName`: String
    public var `devicePublicKey`: ByteString
    public var `idempotencyKey`: Id
    public init(`code`: String, `deviceName`: String, `devicePublicKey`: ByteString, `idempotencyKey`: Id) {
        self.`code` = `code`
        self.`deviceName` = `deviceName`
        self.`devicePublicKey` = `devicePublicKey`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PairBeginResult: Codable, Sendable {
    public var `requestId`: Id
    public var `challenge`: ByteString
    public init(`requestId`: Id, `challenge`: ByteString) {
        self.`requestId` = `requestId`
        self.`challenge` = `challenge`
    }
}

public struct PairProveParams: Codable, Sendable {
    public var `requestId`: Id
    public var `proof`: ByteString
    public var `idempotencyKey`: Id
    public init(`requestId`: Id, `proof`: ByteString, `idempotencyKey`: Id) {
        self.`requestId` = `requestId`
        self.`proof` = `proof`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PairProveResult: Codable, Sendable {
    public var `status`: String
    public init(`status`: String) {
        self.`status` = `status`
    }
}

public struct EngineStatusParams: Codable, Sendable {

    public init() {

    }
}

public struct EngineStatusResult: Codable, Sendable {
    public var `version`: String
    public var `generation`: UInt32
    public var `ptyd`: PtydStatus
    public var `db`: DbStatus
    public init(`version`: String, `generation`: UInt32, `ptyd`: PtydStatus, `db`: DbStatus) {
        self.`version` = `version`
        self.`generation` = `generation`
        self.`ptyd` = `ptyd`
        self.`db` = `db`
    }
}

public struct SettingsGetParams: Codable, Sendable {
    public var `key`: String
    public init(`key`: String) {
        self.`key` = `key`
    }
}

public struct SettingsGetResult: Codable, Sendable {
    public var `key`: String
    public var `value`: JsonValue
    public init(`key`: String, `value`: JsonValue) {
        self.`key` = `key`
        self.`value` = `value`
    }
}

public struct SettingsSetParams: Codable, Sendable {
    public var `key`: String
    public var `value`: JsonValue
    public var `idempotencyKey`: Id
    public init(`key`: String, `value`: JsonValue, `idempotencyKey`: Id) {
        self.`key` = `key`
        self.`value` = `value`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct SettingsSetResult: Codable, Sendable {
    public var `key`: String
    public var `value`: JsonValue
    public init(`key`: String, `value`: JsonValue) {
        self.`key` = `key`
        self.`value` = `value`
    }
}

public struct AuthChallengeEvent: Codable, Sendable {
    public var `nonce`: ByteString
    public var `engineId`: Id
    public init(`nonce`: ByteString, `engineId`: Id) {
        self.`nonce` = `nonce`
        self.`engineId` = `engineId`
    }
}

public struct StateSnapshotPartEvent: Codable, Sendable {
    public var `snapshotId`: Id
    public var `index`: UInt32
    public var `of`: UInt32
    public var `section`: SnapshotSection
    public var `data`: JsonValue
    public init(`snapshotId`: Id, `index`: UInt32, `of`: UInt32, `section`: SnapshotSection, `data`: JsonValue) {
        self.`snapshotId` = `snapshotId`
        self.`index` = `index`
        self.`of` = `of`
        self.`section` = `section`
        self.`data` = `data`
    }
}

public struct WorkspaceCreatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspace`: Workspace
    public init(`eventId`: Decimal, `at`: String, `workspace`: Workspace) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspace` = `workspace`
    }
}

public struct WorkspaceRenamedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspace`: Workspace
    public init(`eventId`: Decimal, `at`: String, `workspace`: Workspace) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspace` = `workspace`
    }
}

public struct WorkspaceClosedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
    }
}

public struct PaneCreatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `pane`: Pane
    public init(`eventId`: Decimal, `at`: String, `pane`: Pane) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`pane` = `pane`
    }
}

public struct PaneRenamedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `pane`: Pane
    public init(`eventId`: Decimal, `at`: String, `pane`: Pane) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`pane` = `pane`
    }
}

public struct PaneClosedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `workspaceId`: Id
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `workspaceId`: Id) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`workspaceId` = `workspaceId`
    }
}

public struct PaneStateChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `state`: PaneState
    public var `confidence`: Double
    public var `phrase`: String
    public var `source`: StateSource
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `state`: PaneState, `confidence`: Double, `phrase`: String, `source`: StateSource) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`state` = `state`
        self.`confidence` = `confidence`
        self.`phrase` = `phrase`
        self.`source` = `source`
    }
}

public struct StreamReplayStartEvent: Codable, Sendable {
    public var `streamId`: Id
    public var `generation`: UInt32
    public var `fromOffset`: Decimal
    public var `toOffset`: Decimal
    public var `reset`: Bool
    public init(`streamId`: Id, `generation`: UInt32, `fromOffset`: Decimal, `toOffset`: Decimal, `reset`: Bool) {
        self.`streamId` = `streamId`
        self.`generation` = `generation`
        self.`fromOffset` = `fromOffset`
        self.`toOffset` = `toOffset`
        self.`reset` = `reset`
    }
}

public struct StreamReplayDoneEvent: Codable, Sendable {
    public var `streamId`: Id
    public var `nextOffset`: Decimal
    public init(`streamId`: Id, `nextOffset`: Decimal) {
        self.`streamId` = `streamId`
        self.`nextOffset` = `nextOffset`
    }
}

public struct StreamGapEvent: Codable, Sendable {
    public var `streamId`: Id
    public var `fromOffset`: Decimal
    public var `toOffset`: Decimal
    public init(`streamId`: Id, `fromOffset`: Decimal, `toOffset`: Decimal) {
        self.`streamId` = `streamId`
        self.`fromOffset` = `fromOffset`
        self.`toOffset` = `toOffset`
    }
}

public struct DevicePairRequestedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `requestId`: Id
    public var `deviceName`: String
    public var `fingerprint`: String
    public init(`eventId`: Decimal, `at`: String, `requestId`: Id, `deviceName`: String, `fingerprint`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`requestId` = `requestId`
        self.`deviceName` = `deviceName`
        self.`fingerprint` = `fingerprint`
    }
}

public struct DevicePairedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `device`: Device
    public init(`eventId`: Decimal, `at`: String, `device`: Device) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`device` = `device`
    }
}

public struct DeviceRevokedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `deviceId`: Id
    public init(`eventId`: Decimal, `at`: String, `deviceId`: Id) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`deviceId` = `deviceId`
    }
}

public struct PairResultEvent: Codable, Sendable {
    public var `requestId`: Id
    public var `approved`: Bool
    public var `deviceToken`: String?
    public var `deviceId`: Id?
    public var `reason`: String?
    public init(`requestId`: Id, `approved`: Bool, `deviceToken`: String? = nil, `deviceId`: Id? = nil, `reason`: String? = nil) {
        self.`requestId` = `requestId`
        self.`approved` = `approved`
        self.`deviceToken` = `deviceToken`
        self.`deviceId` = `deviceId`
        self.`reason` = `reason`
    }
}

public struct EngineUpdatingEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `generation`: UInt32
    public init(`eventId`: Decimal, `at`: String, `generation`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`generation` = `generation`
    }
}

public struct EngineRestartedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `generation`: UInt32
    public init(`eventId`: Decimal, `at`: String, `generation`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`generation` = `generation`
    }
}

public struct PtydReconnectedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `generation`: UInt32
    public init(`eventId`: Decimal, `at`: String, `generation`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`generation` = `generation`
    }
}

public struct PtydLostEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`reason` = `reason`
    }
}

public enum TailSelector: String, Codable, Sendable {
    case `tail` = "tail"
}

public enum RpcVersion: String, Codable, Sendable {
    case `v20` = "2.0"
}

public typealias RpcStringId = String

public typealias RpcNumericId = Int64

public typealias RpcNullId = JsonNull

public enum RpcId: Codable, Sendable {
    case v0(RpcStringId)
    case v1(RpcNumericId)
    case v2(RpcNullId)
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(RpcStringId.self) { self = .v0(value); return }
        if let value = try? container.decode(RpcNumericId.self) { self = .v1(value); return }
        if let value = try? container.decode(RpcNullId.self) { self = .v2(value); return }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "No matching RpcId variant")
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .v0(let value): try container.encode(value)
        case .v1(let value): try container.encode(value)
        case .v2(let value): try container.encode(value)
        }
    }
}

public struct RequestEnvelope: Codable, Sendable {
    public var `jsonrpc`: RpcVersion
    public var `id`: RpcId
    public var `method`: String
    public var `params`: JsonValue
    public init(`jsonrpc`: RpcVersion, `id`: RpcId, `method`: String, `params`: JsonValue) {
        self.`jsonrpc` = `jsonrpc`
        self.`id` = `id`
        self.`method` = `method`
        self.`params` = `params`
    }
}

public struct NotificationEnvelope: Codable, Sendable {
    public var `jsonrpc`: RpcVersion
    public var `method`: String
    public var `params`: JsonValue
    public init(`jsonrpc`: RpcVersion, `method`: String, `params`: JsonValue) {
        self.`jsonrpc` = `jsonrpc`
        self.`method` = `method`
        self.`params` = `params`
    }
}

public struct SuccessEnvelope: Codable, Sendable {
    public var `jsonrpc`: RpcVersion
    public var `id`: RpcId
    public var `result`: JsonValue
    public init(`jsonrpc`: RpcVersion, `id`: RpcId, `result`: JsonValue) {
        self.`jsonrpc` = `jsonrpc`
        self.`id` = `id`
        self.`result` = `result`
    }
}

public struct ErrorEnvelope: Codable, Sendable {
    public var `jsonrpc`: RpcVersion
    public var `id`: RpcId
    public var `error`: RpcError
    public init(`jsonrpc`: RpcVersion, `id`: RpcId, `error`: RpcError) {
        self.`jsonrpc` = `jsonrpc`
        self.`id` = `id`
        self.`error` = `error`
    }
}

public struct PaneUpdatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `pane`: Pane
    public init(`eventId`: Decimal, `at`: String, `pane`: Pane) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`pane` = `pane`
    }
}

public struct SettingsChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `key`: String
    public var `value`: JsonValue
    public init(`eventId`: Decimal, `at`: String, `key`: String, `value`: JsonValue) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`key` = `key`
        self.`value` = `value`
    }
}

public struct DevicePairPending: Codable, Sendable {
    public var `kind`: String
    public var `requestId`: Id
    public var `deviceName`: String
    public var `fingerprint`: String
    public var `expiresAt`: String
    public init(`kind`: String, `requestId`: Id, `deviceName`: String, `fingerprint`: String, `expiresAt`: String) {
        self.`kind` = `kind`
        self.`requestId` = `requestId`
        self.`deviceName` = `deviceName`
        self.`fingerprint` = `fingerprint`
        self.`expiresAt` = `expiresAt`
    }
}

public struct ClientFocus: Codable, Sendable {
    public var `workspaceId`: Id?
    public var `paneId`: Id?
    public init(`workspaceId`: Id? = nil, `paneId`: Id? = nil) {
        self.`workspaceId` = `workspaceId`
        self.`paneId` = `paneId`
    }
}

public enum PermissionMode: String, Codable, Sendable {
    case `ask` = "ask"
    case `accept_edits` = "accept_edits"
    case `allow_all` = "allow_all"
}

public enum PromptOrigin: String, Codable, Sendable {
    case `human` = "human"
    case `automatic` = "automatic"
}

public enum PromptStatus: String, Codable, Sendable {
    case `queued` = "queued"
    case `active` = "active"
    case `completed` = "completed"
    case `cancelled` = "cancelled"
}

public enum TranscriptRole: String, Codable, Sendable {
    case `user` = "user"
    case `assistant` = "assistant"
    case `system` = "system"
}

public enum TranscriptKind: String, Codable, Sendable {
    case `message` = "message"
    case `thought` = "thought"
    case `tool` = "tool"
    case `terminal` = "terminal"
    case `plan` = "plan"
    case `usage` = "usage"
    case `system` = "system"
}

public enum PermissionOptionKind: String, Codable, Sendable {
    case `allow_once` = "allow_once"
    case `allow_always` = "allow_always"
    case `reject_once` = "reject_once"
    case `reject_always` = "reject_always"
}

public enum PermissionResult: String, Codable, Sendable {
    case `selected` = "selected"
    case `cancelled` = "cancelled"
    case `expired` = "expired"
}

public enum ElicitationResult: String, Codable, Sendable {
    case `submitted` = "submitted"
    case `cancelled` = "cancelled"
    case `expired` = "expired"
}

public enum DecisionBy: String, Codable, Sendable {
    case `mac` = "mac"
    case `iphone` = "iphone"
    case `rule` = "rule"
    case `system` = "system"
}

public enum ProbeResult: String, Codable, Sendable {
    case `worked` = "worked"
    case `unsupported` = "unsupported"
    case `unauthenticated` = "unauthenticated"
    case `inconclusive` = "inconclusive"
}

public enum ConfigType: String, Codable, Sendable {
    case `select` = "select"
    case `boolean` = "boolean"
}

public enum ToolStatus: String, Codable, Sendable {
    case `pending` = "pending"
    case `in_progress` = "in_progress"
    case `completed` = "completed"
    case `failed` = "failed"
}

public enum PlanStatus: String, Codable, Sendable {
    case `pending` = "pending"
    case `in_progress` = "in_progress"
    case `completed` = "completed"
}

public enum PlanPriority: String, Codable, Sendable {
    case `high` = "high"
    case `medium` = "medium"
    case `low` = "low"
}

public enum SessionRecovery: String, Codable, Sendable {
    case `resumed` = "resumed"
    case `new_session` = "new_session"
    case `needs_login` = "needs_login"
}

public enum ConfigOperationKind: String, Codable, Sendable {
    case `config` = "config"
    case `yolo` = "yolo"
}

public enum ConfigValue: Codable, Sendable {
    case v0(String)
    case v1(Bool)
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(String.self) { self = .v0(value); return }
        if let value = try? container.decode(Bool.self) { self = .v1(value); return }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "No matching ConfigValue variant")
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .v0(let value): try container.encode(value)
        case .v1(let value): try container.encode(value)
        }
    }
}

public struct ConfigChoice: Codable, Sendable {
    public var `value`: Id
    public var `name`: String
    public var `description`: String?
    public init(`value`: Id, `name`: String, `description`: String? = nil) {
        self.`value` = `value`
        self.`name` = `name`
        self.`description` = `description`
    }
}

public struct ConfigOption: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `description`: String?
    public var `category`: String?
    public var `type`: ConfigType
    public var `currentValue`: ConfigValue
    public var `options`: [ConfigChoice]
    public init(`id`: Id, `name`: String, `description`: String? = nil, `category`: String? = nil, `type`: ConfigType, `currentValue`: ConfigValue, `options`: [ConfigChoice]) {
        self.`id` = `id`
        self.`name` = `name`
        self.`description` = `description`
        self.`category` = `category`
        self.`type` = `type`
        self.`currentValue` = `currentValue`
        self.`options` = `options`
    }
}

public struct ConfigOperation: Codable, Sendable {
    public var `operationId`: Id
    public var `kind`: ConfigOperationKind
    public var `configId`: Id
    public var `value`: ConfigValue
    public var `startedAt`: String
    public init(`operationId`: Id, `kind`: ConfigOperationKind, `configId`: Id, `value`: ConfigValue, `startedAt`: String) {
        self.`operationId` = `operationId`
        self.`kind` = `kind`
        self.`configId` = `configId`
        self.`value` = `value`
        self.`startedAt` = `startedAt`
    }
}

public struct PromptItem: Codable, Sendable {
    public var `promptId`: Id
    public var `text`: String
    public var `origin`: PromptOrigin
    public var `status`: PromptStatus
    public var `queuedAt`: String
    public var `generation`: UInt32?
    public var `attachments`: [AttachmentRef]?
    public init(`promptId`: Id, `text`: String, `origin`: PromptOrigin, `status`: PromptStatus, `queuedAt`: String, `generation`: UInt32? = nil, `attachments`: [AttachmentRef]? = nil) {
        self.`promptId` = `promptId`
        self.`text` = `text`
        self.`origin` = `origin`
        self.`status` = `status`
        self.`queuedAt` = `queuedAt`
        self.`generation` = `generation`
        self.`attachments` = `attachments`
    }
}

public struct PermissionOption: Codable, Sendable {
    public var `optionId`: Id
    public var `kind`: PermissionOptionKind
    public var `name`: String
    public init(`optionId`: Id, `kind`: PermissionOptionKind, `name`: String) {
        self.`optionId` = `optionId`
        self.`kind` = `kind`
        self.`name` = `name`
    }
}

public struct PermissionRequest: Codable, Sendable {
    public var `requestId`: Id
    public var `paneId`: Id
    public var `generation`: UInt32
    public var `promptId`: Id
    public var `title`: String
    public var `detail`: String
    public var `options`: [PermissionOption]
    public var `expiresAt`: String
    public var `kind`: PermissionRequestKind?
    public var `domain`: String?
    public init(`requestId`: Id, `paneId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `detail`: String, `options`: [PermissionOption], `expiresAt`: String, `kind`: PermissionRequestKind? = nil, `domain`: String? = nil) {
        self.`requestId` = `requestId`
        self.`paneId` = `paneId`
        self.`generation` = `generation`
        self.`promptId` = `promptId`
        self.`title` = `title`
        self.`detail` = `detail`
        self.`options` = `options`
        self.`expiresAt` = `expiresAt`
        self.`kind` = `kind`
        self.`domain` = `domain`
    }
}

public struct ElicitationRequest: Codable, Sendable {
    public var `requestId`: Id
    public var `paneId`: Id
    public var `generation`: UInt32
    public var `promptId`: Id
    public var `title`: String
    public var `schema`: JsonValue
    public var `expiresAt`: String
    public init(`requestId`: Id, `paneId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `schema`: JsonValue, `expiresAt`: String) {
        self.`requestId` = `requestId`
        self.`paneId` = `paneId`
        self.`generation` = `generation`
        self.`promptId` = `promptId`
        self.`title` = `title`
        self.`schema` = `schema`
        self.`expiresAt` = `expiresAt`
    }
}

public struct AgentHarnessSwitch: Codable, Sendable {
    public var `fromHarness`: Id
    public var `toHarness`: Id
    public init(`fromHarness`: Id, `toHarness`: Id) {
        self.`fromHarness` = `fromHarness`
        self.`toHarness` = `toHarness`
    }
}

public struct AgentSession: Codable, Sendable {
    public var `harness`: Id
    public var `version`: String
    public var `fingerprint`: String
    public var `sessionId`: Id?
    public var `options`: [ConfigOption]
    public var `authMethods`: [AuthMethod]
    public var `permission`: PermissionMode
    public var `yolo`: Bool
    public var `previousPermission`: PermissionMode?
    public var `effectiveMode`: String?
    public var `operation`: ConfigOperation?
    public var `queue`: [QueuedPromptSummary]
    public var `draft`: Bool
    public var `activePromptId`: Id?
    public var `silentSince`: String?
    public var `queuePaused`: Bool?
    public var `role`: AgentPaneRole
    public var `harnessSwitch`: AgentHarnessSwitch?
    public init(`harness`: Id, `version`: String, `fingerprint`: String, `sessionId`: Id? = nil, `options`: [ConfigOption], `authMethods`: [AuthMethod], `permission`: PermissionMode, `yolo`: Bool, `previousPermission`: PermissionMode? = nil, `effectiveMode`: String? = nil, `operation`: ConfigOperation? = nil, `queue`: [QueuedPromptSummary], `draft`: Bool, `activePromptId`: Id? = nil, `silentSince`: String? = nil, `queuePaused`: Bool? = nil, `role`: AgentPaneRole, `harnessSwitch`: AgentHarnessSwitch? = nil) {
        self.`harness` = `harness`
        self.`version` = `version`
        self.`fingerprint` = `fingerprint`
        self.`sessionId` = `sessionId`
        self.`options` = `options`
        self.`authMethods` = `authMethods`
        self.`permission` = `permission`
        self.`yolo` = `yolo`
        self.`previousPermission` = `previousPermission`
        self.`effectiveMode` = `effectiveMode`
        self.`operation` = `operation`
        self.`queue` = `queue`
        self.`draft` = `draft`
        self.`activePromptId` = `activePromptId`
        self.`silentSince` = `silentSince`
        self.`queuePaused` = `queuePaused`
        self.`role` = `role`
        self.`harnessSwitch` = `harnessSwitch`
    }
}

public struct ProbeCapability: Codable, Sendable {
    public var `method`: String
    public var `result`: ProbeResult
    public var `checkedAt`: String
    public init(`method`: String, `result`: ProbeResult, `checkedAt`: String) {
        self.`method` = `method`
        self.`result` = `result`
        self.`checkedAt` = `checkedAt`
    }
}

public struct AuthMethod: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `description`: String?
    public var `terminal`: Bool
    public init(`id`: Id, `name`: String, `description`: String? = nil, `terminal`: Bool) {
        self.`id` = `id`
        self.`name` = `name`
        self.`description` = `description`
        self.`terminal` = `terminal`
    }
}

public struct HarnessInfo: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `available`: Bool
    public var `installed`: Bool?
    public var `version`: String?
    public var `fingerprint`: String?
    public var `capabilities`: [ProbeCapability]
    public var `options`: [ConfigOption]
    public var `authMethods`: [AuthMethod]
    public var `source`: HarnessSource?
    public var `setupState`: HarnessSetupState?
    public init(`id`: Id, `name`: String, `available`: Bool, `installed`: Bool? = nil, `version`: String? = nil, `fingerprint`: String? = nil, `capabilities`: [ProbeCapability], `options`: [ConfigOption], `authMethods`: [AuthMethod], `source`: HarnessSource? = nil, `setupState`: HarnessSetupState? = nil) {
        self.`id` = `id`
        self.`name` = `name`
        self.`available` = `available`
        self.`installed` = `installed`
        self.`version` = `version`
        self.`fingerprint` = `fingerprint`
        self.`capabilities` = `capabilities`
        self.`options` = `options`
        self.`authMethods` = `authMethods`
        self.`source` = `source`
        self.`setupState` = `setupState`
    }
}

public struct ModelRefusal: Codable, Sendable {
    public var `reason`: String
    public var `observedAt`: String
    public init(`reason`: String, `observedAt`: String) {
        self.`reason` = `reason`
        self.`observedAt` = `observedAt`
    }
}

public struct ModelInfo: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `description`: String?
    public var `observedRefusal`: ModelRefusal?
    public init(`id`: Id, `name`: String, `description`: String? = nil, `observedRefusal`: ModelRefusal? = nil) {
        self.`id` = `id`
        self.`name` = `name`
        self.`description` = `description`
        self.`observedRefusal` = `observedRefusal`
    }
}

public enum ModelListSource: String, Codable, Sendable {
    case `not_probed` = "not_probed"
    case `native_session` = "native_session"
}

public struct HarnessLaunch: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `command`: String
    public var `args`: [String]
    public var `version`: String
    public var `environment`: [HarnessEnvironmentVariable]?
    public init(`id`: Id, `name`: String, `command`: String, `args`: [String], `version`: String, `environment`: [HarnessEnvironmentVariable]? = nil) {
        self.`id` = `id`
        self.`name` = `name`
        self.`command` = `command`
        self.`args` = `args`
        self.`version` = `version`
        self.`environment` = `environment`
    }
}

public typealias HarnessConfiguration = [HarnessLaunch]

public struct ToolLocation: Codable, Sendable {
    public var `path`: String
    public var `line`: UInt32?
    public init(`path`: String, `line`: UInt32? = nil) {
        self.`path` = `path`
        self.`line` = `line`
    }
}

public struct ToolCall: Codable, Sendable {
    public var `toolCallId`: Id
    public var `title`: String
    public var `kind`: String?
    public var `status`: ToolStatus
    public var `content`: JsonValue?
    public var `locations`: [ToolLocation]
    public var `rawInput`: JsonValue?
    public var `rawOutput`: JsonValue?
    public init(`toolCallId`: Id, `title`: String, `kind`: String? = nil, `status`: ToolStatus, `content`: JsonValue? = nil, `locations`: [ToolLocation], `rawInput`: JsonValue? = nil, `rawOutput`: JsonValue? = nil) {
        self.`toolCallId` = `toolCallId`
        self.`title` = `title`
        self.`kind` = `kind`
        self.`status` = `status`
        self.`content` = `content`
        self.`locations` = `locations`
        self.`rawInput` = `rawInput`
        self.`rawOutput` = `rawOutput`
    }
}

public struct PlanEntry: Codable, Sendable {
    public var `content`: String
    public var `priority`: PlanPriority
    public var `status`: PlanStatus
    public init(`content`: String, `priority`: PlanPriority, `status`: PlanStatus) {
        self.`content` = `content`
        self.`priority` = `priority`
        self.`status` = `status`
    }
}

public struct TokenUsage: Codable, Sendable {
    public var `inputTokens`: Decimal?
    public var `outputTokens`: Decimal?
    public var `totalTokens`: Decimal
    public var `contextSize`: Decimal?
    public init(`inputTokens`: Decimal? = nil, `outputTokens`: Decimal? = nil, `totalTokens`: Decimal, `contextSize`: Decimal? = nil) {
        self.`inputTokens` = `inputTokens`
        self.`outputTokens` = `outputTokens`
        self.`totalTokens` = `totalTokens`
        self.`contextSize` = `contextSize`
    }
}

public struct TerminalOutput: Codable, Sendable {
    public var `terminalId`: Id
    public var `bytes`: ByteString
    public var `offset`: Decimal
    public var `truncated`: Bool
    public var `exitCode`: Int64?
    public init(`terminalId`: Id, `bytes`: ByteString, `offset`: Decimal, `truncated`: Bool, `exitCode`: Int64? = nil) {
        self.`terminalId` = `terminalId`
        self.`bytes` = `bytes`
        self.`offset` = `offset`
        self.`truncated` = `truncated`
        self.`exitCode` = `exitCode`
    }
}

public struct TranscriptMessage: Codable, Sendable {
    public var `localId`: Id
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `role`: TranscriptRole
    public var `kind`: TranscriptKind
    public var `text`: String
    public var `complete`: Bool
    public var `promptId`: Id?
    public var `tool`: ToolCall?
    public var `terminal`: TerminalOutput?
    public var `plan`: [PlanEntry]?
    public var `usage`: TokenUsage?
    public init(`localId`: Id, `generation`: UInt32, `seq`: Decimal, `role`: TranscriptRole, `kind`: TranscriptKind, `text`: String, `complete`: Bool, `promptId`: Id? = nil, `tool`: ToolCall? = nil, `terminal`: TerminalOutput? = nil, `plan`: [PlanEntry]? = nil, `usage`: TokenUsage? = nil) {
        self.`localId` = `localId`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`role` = `role`
        self.`kind` = `kind`
        self.`text` = `text`
        self.`complete` = `complete`
        self.`promptId` = `promptId`
        self.`tool` = `tool`
        self.`terminal` = `terminal`
        self.`plan` = `plan`
        self.`usage` = `usage`
    }
}

public struct AttachmentRef: Codable, Sendable {
    public var `attachmentId`: Id
    public var `mimeType`: String
    public var `name`: String
    public var `bytes`: Decimal
    public init(`attachmentId`: Id, `mimeType`: String, `name`: String, `bytes`: Decimal) {
        self.`attachmentId` = `attachmentId`
        self.`mimeType` = `mimeType`
        self.`name` = `name`
        self.`bytes` = `bytes`
    }
}

public struct PanePromptParams: Codable, Sendable {
    public var `paneId`: Id
    public var `text`: String
    public var `origin`: PromptOrigin?
    public var `attachments`: [AttachmentRef]?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `text`: String, `origin`: PromptOrigin? = nil, `attachments`: [AttachmentRef]? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`text` = `text`
        self.`origin` = `origin`
        self.`attachments` = `attachments`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PanePromptResult: Codable, Sendable {
    public var `promptId`: Id
    public var `queued`: Bool
    public init(`promptId`: Id, `queued`: Bool) {
        self.`promptId` = `promptId`
        self.`queued` = `queued`
    }
}

public struct PaneQueueListParams: Codable, Sendable {
    public var `paneId`: Id
    public var `cursor`: Id?
    public var `limit`: UInt32?
    public init(`paneId`: Id, `cursor`: Id? = nil, `limit`: UInt32? = nil) {
        self.`paneId` = `paneId`
        self.`cursor` = `cursor`
        self.`limit` = `limit`
    }
}

public struct PaneQueueListResult: Codable, Sendable {
    public var `items`: [PromptItem]
    public var `draft`: Bool
    public var `nextCursor`: Id?
    public var `paused`: Bool?
    public init(`items`: [PromptItem], `draft`: Bool, `nextCursor`: Id? = nil, `paused`: Bool? = nil) {
        self.`items` = `items`
        self.`draft` = `draft`
        self.`nextCursor` = `nextCursor`
        self.`paused` = `paused`
    }
}

public struct PaneQueueRemoveParams: Codable, Sendable {
    public var `paneId`: Id
    public var `promptId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `promptId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`promptId` = `promptId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneQueueRemoveResult: Codable, Sendable {
    public var `promptId`: Id
    public var `removed`: Bool
    public init(`promptId`: Id, `removed`: Bool) {
        self.`promptId` = `promptId`
        self.`removed` = `removed`
    }
}

public struct PaneCancelParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneCancelResult: Codable, Sendable {
    public var `paneId`: Id
    public var `promptId`: Id?
    public var `cancelling`: Bool
    public init(`paneId`: Id, `promptId`: Id? = nil, `cancelling`: Bool) {
        self.`paneId` = `paneId`
        self.`promptId` = `promptId`
        self.`cancelling` = `cancelling`
    }
}

public struct PanePermissionAnswerParams: Codable, Sendable {
    public var `paneId`: Id
    public var `requestId`: Id
    public var `optionId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `requestId`: Id, `optionId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`optionId` = `optionId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PanePermissionAnswerResult: Codable, Sendable {
    public var `requestId`: Id
    public var `result`: PermissionResult
    public var `optionId`: Id?
    public var `by`: DecisionBy
    public init(`requestId`: Id, `result`: PermissionResult, `optionId`: Id? = nil, `by`: DecisionBy) {
        self.`requestId` = `requestId`
        self.`result` = `result`
        self.`optionId` = `optionId`
        self.`by` = `by`
    }
}

public struct PaneElicitationAnswerParams: Codable, Sendable {
    public var `paneId`: Id
    public var `requestId`: Id
    public var `fields`: JsonValue?
    public var `cancel`: Bool?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `requestId`: Id, `fields`: JsonValue? = nil, `cancel`: Bool? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`fields` = `fields`
        self.`cancel` = `cancel`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneElicitationAnswerResult: Codable, Sendable {
    public var `requestId`: Id
    public var `result`: ElicitationResult
    public var `by`: DecisionBy
    public init(`requestId`: Id, `result`: ElicitationResult, `by`: DecisionBy) {
        self.`requestId` = `requestId`
        self.`result` = `result`
        self.`by` = `by`
    }
}

public struct PaneConfigSetParams: Codable, Sendable {
    public var `paneId`: Id
    public var `configId`: Id
    public var `value`: ConfigValue
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `configId`: Id, `value`: ConfigValue, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`configId` = `configId`
        self.`value` = `value`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneConfigSetResult: Codable, Sendable {
    public var `applied`: Bool
    public var `effective`: ConfigValue
    public var `sessionPreserved`: Bool
    public init(`applied`: Bool, `effective`: ConfigValue, `sessionPreserved`: Bool) {
        self.`applied` = `applied`
        self.`effective` = `effective`
        self.`sessionPreserved` = `sessionPreserved`
    }
}

public struct PaneHarnessSwitchParams: Codable, Sendable {
    public var `paneId`: Id
    public var `harness`: Id
    public var `model`: String?
    public var `effort`: String?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `harness`: Id, `model`: String? = nil, `effort`: String? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`harness` = `harness`
        self.`model` = `model`
        self.`effort` = `effort`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneHarnessSwitchResult: Codable, Sendable {
    public var `paneId`: Id
    public var `fromHarness`: Id
    public var `toHarness`: Id
    public var `newSessionId`: Id
    public var `markerSeq`: Decimal
    public init(`paneId`: Id, `fromHarness`: Id, `toHarness`: Id, `newSessionId`: Id, `markerSeq`: Decimal) {
        self.`paneId` = `paneId`
        self.`fromHarness` = `fromHarness`
        self.`toHarness` = `toHarness`
        self.`newSessionId` = `newSessionId`
        self.`markerSeq` = `markerSeq`
    }
}

public struct PaneHarnessSwitchStartedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `fromHarness`: Id
    public var `toHarness`: Id
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `fromHarness`: Id, `toHarness`: Id) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`fromHarness` = `fromHarness`
        self.`toHarness` = `toHarness`
    }
}

public struct PaneHarnessSwitchDoneEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `fromHarness`: Id
    public var `toHarness`: Id
    public var `newSessionId`: Id
    public var `markerSeq`: Decimal
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `fromHarness`: Id, `toHarness`: Id, `newSessionId`: Id, `markerSeq`: Decimal) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`fromHarness` = `fromHarness`
        self.`toHarness` = `toHarness`
        self.`newSessionId` = `newSessionId`
        self.`markerSeq` = `markerSeq`
    }
}

public struct PaneHarnessSwitchFailedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `fromHarness`: Id
    public var `toHarness`: Id
    public var `reason`: String
    public var `restored`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `fromHarness`: Id, `toHarness`: Id, `reason`: String, `restored`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`fromHarness` = `fromHarness`
        self.`toHarness` = `toHarness`
        self.`reason` = `reason`
        self.`restored` = `restored`
    }
}

public struct PaneYoloParams: Codable, Sendable {
    public var `paneId`: Id
    public var `on`: Bool
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `on`: Bool, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`on` = `on`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneYoloResult: Codable, Sendable {
    public var `effectiveMode`: PermissionMode
    public var `on`: Bool
    public init(`effectiveMode`: PermissionMode, `on`: Bool) {
        self.`effectiveMode` = `effectiveMode`
        self.`on` = `on`
    }
}

public struct PaneAuthStartParams: Codable, Sendable {
    public var `paneId`: Id
    public var `authMethodId`: Id?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `authMethodId`: Id? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`authMethodId` = `authMethodId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneAuthStartResult: Codable, Sendable {
    public var `paneId`: Id
    public var `terminalPaneId`: Id?
    public init(`paneId`: Id, `terminalPaneId`: Id? = nil) {
        self.`paneId` = `paneId`
        self.`terminalPaneId` = `terminalPaneId`
    }
}

public struct PaneTranscriptParams: Codable, Sendable {
    public var `paneId`: Id
    public var `before`: Decimal?
    public var `limit`: UInt32?
    public init(`paneId`: Id, `before`: Decimal? = nil, `limit`: UInt32? = nil) {
        self.`paneId` = `paneId`
        self.`before` = `before`
        self.`limit` = `limit`
    }
}

public struct PaneTranscriptResult: Codable, Sendable {
    public var `items`: [TranscriptMessage]
    public var `nextCursor`: Decimal?
    public init(`items`: [TranscriptMessage], `nextCursor`: Decimal? = nil) {
        self.`items` = `items`
        self.`nextCursor` = `nextCursor`
    }
}

public struct PaneDraftParams: Codable, Sendable {
    public var `paneId`: Id
    public var `active`: Bool
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `active`: Bool, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`active` = `active`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneDraftResult: Codable, Sendable {
    public var `active`: Bool
    public init(`active`: Bool) {
        self.`active` = `active`
    }
}

public struct HarnessListParams: Codable, Sendable {
    public var `cursor`: Id?
    public var `limit`: UInt32?
    public var `rescan`: Bool?
    public init(`cursor`: Id? = nil, `limit`: UInt32? = nil, `rescan`: Bool? = nil) {
        self.`cursor` = `cursor`
        self.`limit` = `limit`
        self.`rescan` = `rescan`
    }
}

public struct HarnessListResult: Codable, Sendable {
    public var `items`: [HarnessInfo]
    public var `nextCursor`: Id?
    public var `environmentIssue`: HarnessEnvironmentIssue?
    public init(`items`: [HarnessInfo], `nextCursor`: Id? = nil, `environmentIssue`: HarnessEnvironmentIssue? = nil) {
        self.`items` = `items`
        self.`nextCursor` = `nextCursor`
        self.`environmentIssue` = `environmentIssue`
    }
}

public struct HarnessCustomListParams: Codable, Sendable {

    public init() {

    }
}

public struct HarnessCustomListResult: Codable, Sendable {
    public var `items`: [HarnessLaunch]
    public init(`items`: [HarnessLaunch]) {
        self.`items` = `items`
    }
}

public struct HarnessCustomCreateParams: Codable, Sendable {
    public var `harness`: HarnessLaunch
    public var `idempotencyKey`: Id
    public init(`harness`: HarnessLaunch, `idempotencyKey`: Id) {
        self.`harness` = `harness`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct HarnessCustomUpdateParams: Codable, Sendable {
    public var `harness`: HarnessLaunch
    public var `idempotencyKey`: Id
    public init(`harness`: HarnessLaunch, `idempotencyKey`: Id) {
        self.`harness` = `harness`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct HarnessCustomMutationResult: Codable, Sendable {
    public var `harness`: HarnessLaunch
    public init(`harness`: HarnessLaunch) {
        self.`harness` = `harness`
    }
}

public struct HarnessCustomRemoveParams: Codable, Sendable {
    public var `harnessId`: Id
    public var `idempotencyKey`: Id
    public init(`harnessId`: Id, `idempotencyKey`: Id) {
        self.`harnessId` = `harnessId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct HarnessCustomRemoveResult: Codable, Sendable {
    public var `harnessId`: Id
    public var `removed`: Bool
    public init(`harnessId`: Id, `removed`: Bool) {
        self.`harnessId` = `harnessId`
        self.`removed` = `removed`
    }
}

public struct ModelListParams: Codable, Sendable {
    public var `harness`: Id
    public var `paneId`: Id?
    public var `cursor`: Id?
    public var `limit`: UInt32?
    public init(`harness`: Id, `paneId`: Id? = nil, `cursor`: Id? = nil, `limit`: UInt32? = nil) {
        self.`harness` = `harness`
        self.`paneId` = `paneId`
        self.`cursor` = `cursor`
        self.`limit` = `limit`
    }
}

public struct ModelListResult: Codable, Sendable {
    public var `items`: [ModelInfo]
    public var `options`: [ConfigOption]
    public var `source`: ModelListSource
    public var `nextCursor`: Id?
    public init(`items`: [ModelInfo], `options`: [ConfigOption], `source`: ModelListSource, `nextCursor`: Id? = nil) {
        self.`items` = `items`
        self.`options` = `options`
        self.`source` = `source`
        self.`nextCursor` = `nextCursor`
    }
}

public struct PaneSessionStartedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `harness`: Id
    public var `version`: String
    public var `sessionId`: Id
    public var `generation`: UInt32
    public var `options`: [ConfigOption]
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `harness`: Id, `version`: String, `sessionId`: Id, `generation`: UInt32, `options`: [ConfigOption]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`harness` = `harness`
        self.`version` = `version`
        self.`sessionId` = `sessionId`
        self.`generation` = `generation`
        self.`options` = `options`
    }
}

public struct PaneTurnEndedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `promptId`: Id
    public var `stopReason`: TurnStopReason
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `promptId`: Id, `stopReason`: TurnStopReason) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`promptId` = `promptId`
        self.`stopReason` = `stopReason`
    }
}

public struct PaneQueuedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `n`: UInt32
    public var `promptIds`: [Id]
    public var `draft`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `n`: UInt32, `promptIds`: [Id], `draft`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`n` = `n`
        self.`promptIds` = `promptIds`
        self.`draft` = `draft`
    }
}

public struct PanePermissionRequestedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `requestId`: Id
    public var `generation`: UInt32
    public var `promptId`: Id
    public var `title`: String
    public var `detail`: String
    public var `options`: [PermissionOption]
    public var `expiresAt`: String
    public var `kind`: PermissionRequestKind?
    public var `domain`: String?
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `requestId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `detail`: String, `options`: [PermissionOption], `expiresAt`: String, `kind`: PermissionRequestKind? = nil, `domain`: String? = nil) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`generation` = `generation`
        self.`promptId` = `promptId`
        self.`title` = `title`
        self.`detail` = `detail`
        self.`options` = `options`
        self.`expiresAt` = `expiresAt`
        self.`kind` = `kind`
        self.`domain` = `domain`
    }
}

public struct PanePermissionAnsweredEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `requestId`: Id
    public var `result`: PermissionResult
    public var `optionId`: Id?
    public var `by`: DecisionBy
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `requestId`: Id, `result`: PermissionResult, `optionId`: Id? = nil, `by`: DecisionBy) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`result` = `result`
        self.`optionId` = `optionId`
        self.`by` = `by`
    }
}

public struct PaneElicitationRequestedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `requestId`: Id
    public var `generation`: UInt32
    public var `promptId`: Id
    public var `title`: String
    public var `schema`: JsonValue
    public var `expiresAt`: String
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `requestId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `schema`: JsonValue, `expiresAt`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`generation` = `generation`
        self.`promptId` = `promptId`
        self.`title` = `title`
        self.`schema` = `schema`
        self.`expiresAt` = `expiresAt`
    }
}

public struct PaneElicitationAnsweredEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `requestId`: Id
    public var `result`: ElicitationResult
    public var `by`: DecisionBy
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `requestId`: Id, `result`: ElicitationResult, `by`: DecisionBy) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`result` = `result`
        self.`by` = `by`
    }
}

public struct PaneConfigChangingEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `configId`: Id
    public var `value`: ConfigValue
    public var `operationId`: Id
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `configId`: Id, `value`: ConfigValue, `operationId`: Id) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`configId` = `configId`
        self.`value` = `value`
        self.`operationId` = `operationId`
    }
}

public struct PaneConfigChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `configId`: Id
    public var `value`: ConfigValue
    public var `options`: [ConfigOption]
    public var `sessionPreserved`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `configId`: Id, `value`: ConfigValue, `options`: [ConfigOption], `sessionPreserved`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`configId` = `configId`
        self.`value` = `value`
        self.`options` = `options`
        self.`sessionPreserved` = `sessionPreserved`
    }
}

public struct PaneConfigFailedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `configId`: Id
    public var `value`: ConfigValue
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `configId`: Id, `value`: ConfigValue, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`configId` = `configId`
        self.`value` = `value`
        self.`reason` = `reason`
    }
}

public struct PaneSessionLostEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `reason`: String
    public var `recovery`: SessionRecovery
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `reason`: String, `recovery`: SessionRecovery) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`reason` = `reason`
        self.`recovery` = `recovery`
    }
}

public struct PaneNeedsLoginEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `harness`: Id
    public var `authMethods`: [AuthMethod]
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `harness`: Id, `authMethods`: [AuthMethod]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`harness` = `harness`
        self.`authMethods` = `authMethods`
    }
}

public struct PaneAuthFinishedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `ok`: Bool
    public var `reason`: String?
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `ok`: Bool, `reason`: String? = nil) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`ok` = `ok`
        self.`reason` = `reason`
    }
}

public enum AgentMessageUpdateKind: String, Codable, Sendable {
    case `agent_message_chunk` = "agent_message_chunk"
}

public struct AgentMessageUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: AgentMessageUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `text`: String
    public var `complete`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: AgentMessageUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `text`: String, `complete`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`text` = `text`
        self.`complete` = `complete`
    }
}

public enum AgentThoughtUpdateKind: String, Codable, Sendable {
    case `agent_thought_chunk` = "agent_thought_chunk"
}

public struct AgentThoughtUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: AgentThoughtUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `text`: String
    public var `complete`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: AgentThoughtUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `text`: String, `complete`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`text` = `text`
        self.`complete` = `complete`
    }
}

public enum ToolCallUpdateKind: String, Codable, Sendable {
    case `tool_call` = "tool_call"
}

public struct ToolCallUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: ToolCallUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `tool`: ToolCall
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: ToolCallUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `tool`: ToolCall) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`tool` = `tool`
    }
}

public enum ToolProgressUpdateKind: String, Codable, Sendable {
    case `tool_call_update` = "tool_call_update"
}

public struct ToolProgressUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: ToolProgressUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `tool`: ToolCall
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: ToolProgressUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `tool`: ToolCall) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`tool` = `tool`
    }
}

public enum PlanUpdateKind: String, Codable, Sendable {
    case `plan` = "plan"
}

public struct PlanUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: PlanUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `entries`: [PlanEntry]
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: PlanUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `entries`: [PlanEntry]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`entries` = `entries`
    }
}

public enum UsageUpdateKind: String, Codable, Sendable {
    case `usage` = "usage"
}

public struct UsageUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: UsageUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `usage`: TokenUsage
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: UsageUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `usage`: TokenUsage) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`usage` = `usage`
    }
}

public enum TerminalOutputUpdateKind: String, Codable, Sendable {
    case `terminal_output` = "terminal_output"
}

public struct TerminalOutputUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: TerminalOutputUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `terminalId`: Id
    public var `bytes`: ByteString
    public var `offset`: Decimal
    public var `truncated`: Bool
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: TerminalOutputUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `terminalId`: Id, `bytes`: ByteString, `offset`: Decimal, `truncated`: Bool) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`terminalId` = `terminalId`
        self.`bytes` = `bytes`
        self.`offset` = `offset`
        self.`truncated` = `truncated`
    }
}

public enum ConfigOptionUpdateKind: String, Codable, Sendable {
    case `config_option_update` = "config_option_update"
}

public struct ConfigOptionUpdate: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `kind`: ConfigOptionUpdateKind
    public var `generation`: UInt32
    public var `seq`: Decimal
    public var `localId`: Id
    public var `options`: [ConfigOption]
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `kind`: ConfigOptionUpdateKind, `generation`: UInt32, `seq`: Decimal, `localId`: Id, `options`: [ConfigOption]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`kind` = `kind`
        self.`generation` = `generation`
        self.`seq` = `seq`
        self.`localId` = `localId`
        self.`options` = `options`
    }
}

public enum PaneUpdateEvent: Codable, Sendable {
    case v0(AgentMessageUpdate)
    case v1(AgentThoughtUpdate)
    case v2(ToolCallUpdate)
    case v3(ToolProgressUpdate)
    case v4(PlanUpdate)
    case v5(UsageUpdate)
    case v6(TerminalOutputUpdate)
    case v7(ConfigOptionUpdate)
    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(AgentMessageUpdate.self) { self = .v0(value); return }
        if let value = try? container.decode(AgentThoughtUpdate.self) { self = .v1(value); return }
        if let value = try? container.decode(ToolCallUpdate.self) { self = .v2(value); return }
        if let value = try? container.decode(ToolProgressUpdate.self) { self = .v3(value); return }
        if let value = try? container.decode(PlanUpdate.self) { self = .v4(value); return }
        if let value = try? container.decode(UsageUpdate.self) { self = .v5(value); return }
        if let value = try? container.decode(TerminalOutputUpdate.self) { self = .v6(value); return }
        if let value = try? container.decode(ConfigOptionUpdate.self) { self = .v7(value); return }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "No matching PaneUpdateEvent variant")
    }
    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .v0(let value): try container.encode(value)
        case .v1(let value): try container.encode(value)
        case .v2(let value): try container.encode(value)
        case .v3(let value): try container.encode(value)
        case .v4(let value): try container.encode(value)
        case .v5(let value): try container.encode(value)
        case .v6(let value): try container.encode(value)
        case .v7(let value): try container.encode(value)
        }
    }
}

public struct AttentionItem: Codable, Sendable {
    public var `attentionId`: Id
    public var `paneId`: Id
    public var `requestId`: Id?
    public var `message`: String
    public var `createdAt`: String
    public init(`attentionId`: Id, `paneId`: Id, `requestId`: Id? = nil, `message`: String, `createdAt`: String) {
        self.`attentionId` = `attentionId`
        self.`paneId` = `paneId`
        self.`requestId` = `requestId`
        self.`message` = `message`
        self.`createdAt` = `createdAt`
    }
}

public struct AttentionChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `count`: UInt32
    public var `items`: [AttentionItem]
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `count`: UInt32, `items`: [AttentionItem]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`count` = `count`
        self.`items` = `items`
    }
}

public enum TurnStopReason: String, Codable, Sendable {
    case `end_turn` = "end_turn"
    case `max_tokens` = "max_tokens"
    case `max_turn_requests` = "max_turn_requests"
    case `refusal` = "refusal"
    case `cancelled` = "cancelled"
    case `error` = "error"
}

public struct QueuedPromptSummary: Codable, Sendable {
    public var `promptId`: Id
    public var `origin`: PromptOrigin
    public var `status`: PromptStatus
    public var `queuedAt`: String
    public var `generation`: UInt32?
    public init(`promptId`: Id, `origin`: PromptOrigin, `status`: PromptStatus, `queuedAt`: String, `generation`: UInt32? = nil) {
        self.`promptId` = `promptId`
        self.`origin` = `origin`
        self.`status` = `status`
        self.`queuedAt` = `queuedAt`
        self.`generation` = `generation`
    }
}

public struct PaneQueueResumeParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneQueueResumeResult: Codable, Sendable {
    public var `paneId`: Id
    public var `paused`: Bool
    public init(`paneId`: Id, `paused`: Bool) {
        self.`paneId` = `paneId`
        self.`paused` = `paused`
    }
}

public struct PaneQueueStateEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `paused`: Bool
    public var `reason`: String?
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `paused`: Bool, `reason`: String? = nil) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`paused` = `paused`
        self.`reason` = `reason`
    }
}

public enum TaskState: String, Codable, Sendable {
    case `todo` = "todo"
    case `in_progress` = "in_progress"
    case `in_review` = "in_review"
    case `completed` = "completed"
    case `cancelled` = "cancelled"
}

public enum TaskPriority: String, Codable, Sendable {
    case `urgent` = "urgent"
    case `high` = "high"
    case `medium` = "medium"
    case `low` = "low"
}

public enum SubitemState: String, Codable, Sendable {
    case `open` = "open"
    case `done` = "done"
}

public enum AttemptResult: String, Codable, Sendable {
    case `completed` = "completed"
    case `returned` = "returned"
    case `failed` = "failed"
    case `cancelled` = "cancelled"
}

public enum BlockingKind: String, Codable, Sendable {
    case `dependency` = "dependency"
    case `missing_information` = "missing_information"
    case `external_error` = "external_error"
}

public enum TaskCheckState: String, Codable, Sendable {
    case `unknown` = "unknown"
    case `pending` = "pending"
    case `green` = "green"
    case `red` = "red"
    case `conflict` = "conflict"
}

public enum TaskReviewState: String, Codable, Sendable {
    case `none` = "none"
    case `awaiting_person` = "awaiting_person"
    case `changes_requested` = "changes_requested"
    case `approved` = "approved"
}

public enum TaskReviewBadge: String, Codable, Sendable {
    case `system_turn` = "system_turn"
    case `your_turn` = "your_turn"
}

public enum LoopStopWhen: String, Codable, Sendable {
    case `round_limit` = "round_limit"
    case `three_empty` = "three_empty"
    case `explicit_off` = "explicit_off"
}

public enum DriftLabel: String, Codable, Sendable {
    case `equivalent` = "equivalent"
    case `style` = "style"
    case `algorithm` = "algorithm"
    case `input_gating` = "input_gating"
    case `silent_fallback` = "silent_fallback"
    case `error_handling` = "error_handling"
    case `return_shape` = "return_shape"
    case `external_effects` = "external_effects"
    case `extra_edits` = "extra_edits"
    case `incomplete` = "incomplete"
}

public struct TaskExecutor: Codable, Sendable {
    public var `workspaceDefault`: Bool
    public var `harness`: Id?
    public var `model`: String?
    public var `effort`: String?
    public var `permission`: PermissionMode?
    public init(`workspaceDefault`: Bool, `harness`: Id? = nil, `model`: String? = nil, `effort`: String? = nil, `permission`: PermissionMode? = nil) {
        self.`workspaceDefault` = `workspaceDefault`
        self.`harness` = `harness`
        self.`model` = `model`
        self.`effort` = `effort`
        self.`permission` = `permission`
    }
}

public struct TaskWorktree: Codable, Sendable {
    public var `path`: String
    public var `branch`: String
    public init(`path`: String, `branch`: String) {
        self.`path` = `path`
        self.`branch` = `branch`
    }
}

public struct TaskPullRequest: Codable, Sendable {
    public var `url`: String
    public var `checkState`: TaskCheckState
    public var `reviewState`: TaskReviewState
    public var `head`: String?
    public init(`url`: String, `checkState`: TaskCheckState, `reviewState`: TaskReviewState, `head`: String? = nil) {
        self.`url` = `url`
        self.`checkState` = `checkState`
        self.`reviewState` = `reviewState`
        self.`head` = `head`
    }
}

public struct Blocking: Codable, Sendable {
    public var `kind`: BlockingKind
    public var `reason`: String
    public var `dependencyTaskId`: Id?
    public var `retryCount`: UInt32?
    public init(`kind`: BlockingKind, `reason`: String, `dependencyTaskId`: Id? = nil, `retryCount`: UInt32? = nil) {
        self.`kind` = `kind`
        self.`reason` = `reason`
        self.`dependencyTaskId` = `dependencyTaskId`
        self.`retryCount` = `retryCount`
    }
}

public struct Subitem: Codable, Sendable {
    public var `subitemId`: Id
    public var `taskId`: Id
    public var `text`: String
    public var `agent`: Id?
    public var `instructions`: String?
    public var `state`: SubitemState
    public init(`subitemId`: Id, `taskId`: Id, `text`: String, `agent`: Id? = nil, `instructions`: String? = nil, `state`: SubitemState) {
        self.`subitemId` = `subitemId`
        self.`taskId` = `taskId`
        self.`text` = `text`
        self.`agent` = `agent`
        self.`instructions` = `instructions`
        self.`state` = `state`
    }
}

public struct Comment: Codable, Sendable {
    public var `commentId`: Id
    public var `taskId`: Id
    public var `author`: String
    public var `text`: String
    public var `at`: String
    public init(`commentId`: Id, `taskId`: Id, `author`: String, `text`: String, `at`: String) {
        self.`commentId` = `commentId`
        self.`taskId` = `taskId`
        self.`author` = `author`
        self.`text` = `text`
        self.`at` = `at`
    }
}

public struct Attempt: Codable, Sendable {
    public var `n`: UInt32
    public var `paneId`: Id
    public var `harness`: Id
    public var `model`: String
    public var `start`: String
    public var `end`: String?
    public var `result`: AttemptResult?
    public var `summary`: String?
    public init(`n`: UInt32, `paneId`: Id, `harness`: Id, `model`: String, `start`: String, `end`: String? = nil, `result`: AttemptResult? = nil, `summary`: String? = nil) {
        self.`n` = `n`
        self.`paneId` = `paneId`
        self.`harness` = `harness`
        self.`model` = `model`
        self.`start` = `start`
        self.`end` = `end`
        self.`result` = `result`
        self.`summary` = `summary`
    }
}

public struct TaskMove: Codable, Sendable {
    public var `actor`: String
    public var `from`: TaskState
    public var `to`: TaskState
    public var `reason`: String
    public var `at`: String
    public init(`actor`: String, `from`: TaskState, `to`: TaskState, `reason`: String, `at`: String) {
        self.`actor` = `actor`
        self.`from` = `from`
        self.`to` = `to`
        self.`reason` = `reason`
        self.`at` = `at`
    }
}

public struct ContractChange: Codable, Sendable {
    public var `path`: String
    public var `name`: String
    public var `label`: DriftLabel
    public var `contractRef`: String
    public init(`path`: String, `name`: String, `label`: DriftLabel, `contractRef`: String) {
        self.`path` = `path`
        self.`name` = `name`
        self.`label` = `label`
        self.`contractRef` = `contractRef`
    }
}

public struct OutOfScopeEdit: Codable, Sendable {
    public var `path`: String
    public var `reason`: String
    public init(`path`: String, `reason`: String) {
        self.`path` = `path`
        self.`reason` = `reason`
    }
}

public struct LoopConfig: Codable, Sendable {
    public var `prompt`: String
    public var `interval`: UInt32
    public var `maxRounds`: UInt32
    public var `stopWhen`: LoopStopWhen
    public var `enabled`: Bool
    public var `rounds`: UInt32?
    public var `emptyResults`: UInt32?
    public init(`prompt`: String, `interval`: UInt32, `maxRounds`: UInt32, `stopWhen`: LoopStopWhen, `enabled`: Bool, `rounds`: UInt32? = nil, `emptyResults`: UInt32? = nil) {
        self.`prompt` = `prompt`
        self.`interval` = `interval`
        self.`maxRounds` = `maxRounds`
        self.`stopWhen` = `stopWhen`
        self.`enabled` = `enabled`
        self.`rounds` = `rounds`
        self.`emptyResults` = `emptyResults`
    }
}

public struct TaskRecord: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `title`: String
    public var `description`: String
    public var `howToVerify`: String?
    public var `mustNotChange`: String
    public var `state`: TaskState
    public var `priority`: TaskPriority
    public var `plannedOwner`: Id?
    public var `ownerPaneId`: Id?
    public var `executor`: TaskExecutor?
    public var `dependencies`: [String]
    public var `worktree`: TaskWorktree?
    public var `currentAttempt`: UInt32
    public var `blocking`: Blocking?
    public var `pr`: TaskPullRequest?
    public var `prContract`: Bool
    public var `createdAt`: String
    public var `createdBy`: String
    public var `version`: UInt32
    public var `phrase`: String
    public var `reviewBadge`: TaskReviewBadge?
    public var `summary`: String?
    public var `contractChanges`: [ContractChange]?
    public var `outOfScopeEdits`: [OutOfScopeEdit]?
    public init(`workspaceId`: Id, `taskId`: Id, `title`: String, `description`: String, `howToVerify`: String? = nil, `mustNotChange`: String, `state`: TaskState, `priority`: TaskPriority, `plannedOwner`: Id? = nil, `ownerPaneId`: Id? = nil, `executor`: TaskExecutor? = nil, `dependencies`: [String], `worktree`: TaskWorktree? = nil, `currentAttempt`: UInt32, `blocking`: Blocking? = nil, `pr`: TaskPullRequest? = nil, `prContract`: Bool, `createdAt`: String, `createdBy`: String, `version`: UInt32, `phrase`: String, `reviewBadge`: TaskReviewBadge? = nil, `summary`: String? = nil, `contractChanges`: [ContractChange]? = nil, `outOfScopeEdits`: [OutOfScopeEdit]? = nil) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`title` = `title`
        self.`description` = `description`
        self.`howToVerify` = `howToVerify`
        self.`mustNotChange` = `mustNotChange`
        self.`state` = `state`
        self.`priority` = `priority`
        self.`plannedOwner` = `plannedOwner`
        self.`ownerPaneId` = `ownerPaneId`
        self.`executor` = `executor`
        self.`dependencies` = `dependencies`
        self.`worktree` = `worktree`
        self.`currentAttempt` = `currentAttempt`
        self.`blocking` = `blocking`
        self.`pr` = `pr`
        self.`prContract` = `prContract`
        self.`createdAt` = `createdAt`
        self.`createdBy` = `createdBy`
        self.`version` = `version`
        self.`phrase` = `phrase`
        self.`reviewBadge` = `reviewBadge`
        self.`summary` = `summary`
        self.`contractChanges` = `contractChanges`
        self.`outOfScopeEdits` = `outOfScopeEdits`
    }
}

public struct TaskCreateParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `title`: String
    public var `description`: String?
    public var `howToVerify`: String?
    public var `mustNotChange`: String
    public var `executor`: TaskExecutor?
    public var `priority`: TaskPriority?
    public var `createWorktree`: Bool?
    public var `permission`: PermissionMode?
    public var `dependencies`: [String]?
    public var `dispatch`: Bool?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `title`: String, `description`: String? = nil, `howToVerify`: String? = nil, `mustNotChange`: String, `executor`: TaskExecutor? = nil, `priority`: TaskPriority? = nil, `createWorktree`: Bool? = nil, `permission`: PermissionMode? = nil, `dependencies`: [String]? = nil, `dispatch`: Bool? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`title` = `title`
        self.`description` = `description`
        self.`howToVerify` = `howToVerify`
        self.`mustNotChange` = `mustNotChange`
        self.`executor` = `executor`
        self.`priority` = `priority`
        self.`createWorktree` = `createWorktree`
        self.`permission` = `permission`
        self.`dependencies` = `dependencies`
        self.`dispatch` = `dispatch`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskCreateResult: Codable, Sendable {
    public var `task`: TaskRecord
    public var `paneId`: Id?
    public init(`task`: TaskRecord, `paneId`: Id? = nil) {
        self.`task` = `task`
        self.`paneId` = `paneId`
    }
}

public struct TaskNextParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `paneId`: Id
    public var `sessionGeneration`: UInt32
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `paneId`: Id, `sessionGeneration`: UInt32, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskNextResult: Codable, Sendable {
    public var `task`: TaskRecord?
    public var `empty`: Bool
    public var `willRestartSession`: Bool
    public init(`task`: TaskRecord? = nil, `empty`: Bool, `willRestartSession`: Bool) {
        self.`task` = `task`
        self.`empty` = `empty`
        self.`willRestartSession` = `willRestartSession`
    }
}

public struct TaskShowParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public init(`workspaceId`: Id, `taskId`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
    }
}

public struct TaskShowResult: Codable, Sendable {
    public var `task`: TaskRecord
    public var `subitems`: [Subitem]
    public var `comments`: [Comment]
    public var `attempts`: [Attempt]
    public var `history`: [TaskMove]
    public init(`task`: TaskRecord, `subitems`: [Subitem], `comments`: [Comment], `attempts`: [Attempt], `history`: [TaskMove]) {
        self.`task` = `task`
        self.`subitems` = `subitems`
        self.`comments` = `comments`
        self.`attempts` = `attempts`
        self.`history` = `history`
    }
}

public struct TaskListParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `state`: TaskState?
    public var `limit`: UInt32
    public var `cursor`: Id?
    public init(`workspaceId`: Id, `state`: TaskState? = nil, `limit`: UInt32, `cursor`: Id? = nil) {
        self.`workspaceId` = `workspaceId`
        self.`state` = `state`
        self.`limit` = `limit`
        self.`cursor` = `cursor`
    }
}

public struct TaskListResult: Codable, Sendable {
    public var `items`: [TaskRecord]
    public var `nextCursor`: Id?
    public init(`items`: [TaskRecord], `nextCursor`: Id? = nil) {
        self.`items` = `items`
        self.`nextCursor` = `nextCursor`
    }
}

public struct TaskCommentsParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `before`: Id?
    public var `limit`: UInt32
    public init(`workspaceId`: Id, `taskId`: Id, `before`: Id? = nil, `limit`: UInt32) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`before` = `before`
        self.`limit` = `limit`
    }
}

public struct TaskCommentsResult: Codable, Sendable {
    public var `items`: [Comment]
    public var `nextCursor`: Id?
    public init(`items`: [Comment], `nextCursor`: Id? = nil) {
        self.`items` = `items`
        self.`nextCursor` = `nextCursor`
    }
}

public struct TaskUpdateParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `version`: UInt32
    public var `title`: String?
    public var `description`: String?
    public var `howToVerify`: String?
    public var `mustNotChange`: String?
    public var `priority`: TaskPriority?
    public var `dependencies`: [String]?
    public var `plannedOwner`: Id?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `version`: UInt32, `title`: String? = nil, `description`: String? = nil, `howToVerify`: String? = nil, `mustNotChange`: String? = nil, `priority`: TaskPriority? = nil, `dependencies`: [String]? = nil, `plannedOwner`: Id? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`version` = `version`
        self.`title` = `title`
        self.`description` = `description`
        self.`howToVerify` = `howToVerify`
        self.`mustNotChange` = `mustNotChange`
        self.`priority` = `priority`
        self.`dependencies` = `dependencies`
        self.`plannedOwner` = `plannedOwner`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskUpdateResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskCommentParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `text`: String
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `text`: String, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`text` = `text`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskCommentResult: Codable, Sendable {
    public var `comment`: Comment
    public init(`comment`: Comment) {
        self.`comment` = `comment`
    }
}

public struct TaskSubitemAddParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `text`: String
    public var `agent`: Id?
    public var `instructions`: String?
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `text`: String, `agent`: Id? = nil, `instructions`: String? = nil, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`text` = `text`
        self.`agent` = `agent`
        self.`instructions` = `instructions`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskSubitemAddResult: Codable, Sendable {
    public var `subitem`: Subitem
    public init(`subitem`: Subitem) {
        self.`subitem` = `subitem`
    }
}

public struct TaskSubitemDoneParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `subitemId`: Id
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `subitemId`: Id, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`subitemId` = `subitemId`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskSubitemDoneResult: Codable, Sendable {
    public var `subitem`: Subitem
    public init(`subitem`: Subitem) {
        self.`subitem` = `subitem`
    }
}

public struct TaskBlockParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `kind`: BlockingKind
    public var `reason`: String
    public var `dependencyTaskId`: Id?
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `kind`: BlockingKind, `reason`: String, `dependencyTaskId`: Id? = nil, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`kind` = `kind`
        self.`reason` = `reason`
        self.`dependencyTaskId` = `dependencyTaskId`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskBlockResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskReleaseParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `reason`: String?
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `reason`: String? = nil, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`reason` = `reason`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskReleaseResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskRequestReviewParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `reason`: String?
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `reason`: String? = nil, `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`reason` = `reason`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskRequestReviewResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskCompleteParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `summary`: String
    public var `contractChanges`: [ContractChange]
    public var `outOfScopeEdits`: [OutOfScopeEdit]
    public var `paneId`: Id?
    public var `sessionGeneration`: UInt32?
    public var `attempt`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `summary`: String, `contractChanges`: [ContractChange], `outOfScopeEdits`: [OutOfScopeEdit], `paneId`: Id? = nil, `sessionGeneration`: UInt32? = nil, `attempt`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`summary` = `summary`
        self.`contractChanges` = `contractChanges`
        self.`outOfScopeEdits` = `outOfScopeEdits`
        self.`paneId` = `paneId`
        self.`sessionGeneration` = `sessionGeneration`
        self.`attempt` = `attempt`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskCompleteResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskDispatchParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `preset`: TaskExecutor
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `preset`: TaskExecutor, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`preset` = `preset`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskDispatchResult: Codable, Sendable {
    public var `task`: TaskRecord
    public var `paneId`: Id
    public var `attempt`: UInt32
    public var `resumed`: Bool
    public init(`task`: TaskRecord, `paneId`: Id, `attempt`: UInt32, `resumed`: Bool) {
        self.`task` = `task`
        self.`paneId` = `paneId`
        self.`attempt` = `attempt`
        self.`resumed` = `resumed`
    }
}

public struct TaskUnblockParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `reason`: String?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `reason`: String? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`reason` = `reason`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskUnblockResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskMoveParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `state`: TaskState
    public var `reason`: String
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `state`: TaskState, `reason`: String, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`state` = `state`
        self.`reason` = `reason`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskMoveResult: Codable, Sendable {
    public var `task`: TaskRecord
    public init(`task`: TaskRecord) {
        self.`task` = `task`
    }
}

public struct TaskWorktreeCleanParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `taskId`: Id, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct TaskWorktreeCleanResult: Codable, Sendable {
    public var `cleaned`: Bool
    public var `reason`: String?
    public init(`cleaned`: Bool, `reason`: String? = nil) {
        self.`cleaned` = `cleaned`
        self.`reason` = `reason`
    }
}

public struct LoopSetParams: Codable, Sendable {
    public var `paneId`: Id
    public var `prompt`: String
    public var `interval`: UInt32
    public var `maxRounds`: UInt32
    public var `stopWhen`: LoopStopWhen
    public var `enabled`: Bool
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `prompt`: String, `interval`: UInt32, `maxRounds`: UInt32, `stopWhen`: LoopStopWhen, `enabled`: Bool, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`prompt` = `prompt`
        self.`interval` = `interval`
        self.`maxRounds` = `maxRounds`
        self.`stopWhen` = `stopWhen`
        self.`enabled` = `enabled`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct LoopSetResult: Codable, Sendable {
    public var `config`: LoopConfig
    public init(`config`: LoopConfig) {
        self.`config` = `config`
    }
}

public struct TaskCreatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `task`: TaskRecord
    public init(`eventId`: Decimal, `at`: String, `task`: TaskRecord) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`task` = `task`
    }
}

public struct TaskMovedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `actor`: String
    public var `from`: TaskState
    public var `to`: TaskState
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `actor`: String, `from`: TaskState, `to`: TaskState, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`actor` = `actor`
        self.`from` = `from`
        self.`to` = `to`
        self.`reason` = `reason`
    }
}

public struct TaskClaimedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `paneId`: Id
    public var `attempt`: UInt32
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `paneId`: Id, `attempt`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`paneId` = `paneId`
        self.`attempt` = `attempt`
    }
}

public struct TaskReleasedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `reason`: String
    public var `attempt`: UInt32
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `reason`: String, `attempt`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`reason` = `reason`
        self.`attempt` = `attempt`
    }
}

public struct TaskCommentedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `comment`: Comment
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `comment`: Comment) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`comment` = `comment`
    }
}

public struct TaskSubitemChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `subitem`: Subitem
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `subitem`: Subitem) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`subitem` = `subitem`
    }
}

public struct TaskBlockedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `kind`: BlockingKind
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `kind`: BlockingKind, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`kind` = `kind`
        self.`reason` = `reason`
    }
}

public struct TaskUnblockedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`reason` = `reason`
    }
}

public struct TaskPhraseChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `phrase`: String
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `phrase`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`phrase` = `phrase`
    }
}

public struct TaskUpdatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `version`: UInt32
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `version`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`version` = `version`
    }
}

public struct TaskAttentionEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `workspaceId`: Id
    public var `taskId`: Id
    public var `n`: UInt32
    public init(`eventId`: Decimal, `at`: String, `workspaceId`: Id, `taskId`: Id, `n`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`workspaceId` = `workspaceId`
        self.`taskId` = `taskId`
        self.`n` = `n`
    }
}

public struct LoopTickEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `round`: UInt32
    public var `of`: UInt32
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `round`: UInt32, `of`: UInt32) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`round` = `round`
        self.`of` = `of`
    }
}

public struct LoopStoppedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `reason`: String
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `reason`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`reason` = `reason`
    }
}

public struct PaneRoleSetParams: Codable, Sendable {
    public var `paneId`: Id
    public var `role`: AgentPaneRole
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `role`: AgentPaneRole, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`role` = `role`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct PaneRoleSetResult: Codable, Sendable {
    public var `paneId`: Id
    public var `role`: AgentPaneRole
    public init(`paneId`: Id, `role`: AgentPaneRole) {
        self.`paneId` = `paneId`
        self.`role` = `role`
    }
}

public enum UsageProvider: String, Codable, Sendable {
    case `claude` = "claude"
    case `codex` = "codex"
    case `cursor` = "cursor"
}

public enum UsageState: String, Codable, Sendable {
    case `ok` = "ok"
    case `stale` = "stale"
    case `login_needed` = "login_needed"
    case `no_access` = "no_access"
    case `unavailable` = "unavailable"
}

public enum UsageSource: String, Codable, Sendable {
    case `claude_cli_usage` = "claude_cli_usage"
    case `claude_oauth_endpoint` = "claude_oauth_endpoint"
    case `claude_desktop_cache` = "claude_desktop_cache"
    case `codex_app_server` = "codex_app_server"
    case `codex_account_endpoint` = "codex_account_endpoint"
    case `cursor_current_period` = "cursor_current_period"
}

public struct UsageWindow: Codable, Sendable {
    public var `used`: Double
    public var `resetsAt`: String
    public var `label`: String
    public init(`used`: Double, `resetsAt`: String, `label`: String) {
        self.`used` = `used`
        self.`resetsAt` = `resetsAt`
        self.`label` = `label`
    }
}

public struct UsageReading: Codable, Sendable {
    public var `provider`: UsageProvider
    public var `state`: UsageState
    public var `primary`: UsageWindow?
    public var `secondary`: UsageWindow?
    public var `source`: UsageSource
    public var `fetchedAt`: String
    public init(`provider`: UsageProvider, `state`: UsageState, `primary`: UsageWindow? = nil, `secondary`: UsageWindow? = nil, `source`: UsageSource, `fetchedAt`: String) {
        self.`provider` = `provider`
        self.`state` = `state`
        self.`primary` = `primary`
        self.`secondary` = `secondary`
        self.`source` = `source`
        self.`fetchedAt` = `fetchedAt`
    }
}

public struct UsageListParams: Codable, Sendable {

    public init() {

    }
}

public struct UsageListResult: Codable, Sendable {
    public var `providers`: [UsageReading]
    public init(`providers`: [UsageReading]) {
        self.`providers` = `providers`
    }
}

public struct UsageRefreshParams: Codable, Sendable {
    public var `provider`: UsageProvider?
    public var `idempotencyKey`: Id
    public init(`provider`: UsageProvider? = nil, `idempotencyKey`: Id) {
        self.`provider` = `provider`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct UsageRefreshResult: Codable, Sendable {
    public var `providers`: [UsageReading]
    public init(`providers`: [UsageReading]) {
        self.`providers` = `providers`
    }
}

public struct UsageChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `provider`: UsageProvider
    public var `state`: UsageState
    public var `primary`: UsageWindow?
    public var `secondary`: UsageWindow?
    public var `source`: UsageSource
    public var `fetchedAt`: String
    public init(`eventId`: Decimal, `at`: String, `provider`: UsageProvider, `state`: UsageState, `primary`: UsageWindow? = nil, `secondary`: UsageWindow? = nil, `source`: UsageSource, `fetchedAt`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`provider` = `provider`
        self.`state` = `state`
        self.`primary` = `primary`
        self.`secondary` = `secondary`
        self.`source` = `source`
        self.`fetchedAt` = `fetchedAt`
    }
}

public enum EditorLanguage: String, Codable, Sendable {
    case `plain_text` = "plain_text"
    case `rust` = "rust"
    case `swift` = "swift"
    case `typescript` = "typescript"
    case `python` = "python"
    case `json` = "json"
    case `markdown` = "markdown"
    case `shell` = "shell"
}

public enum EditorContentSource: String, Codable, Sendable {
    case `buffer` = "buffer"
    case `disk` = "disk"
    case `draft` = "draft"
    case `history` = "history"
}

public enum FileSearchScope: String, Codable, Sendable {
    case `content` = "content"
    case `path` = "path"
}

public enum HighlightKind: String, Codable, Sendable {
    case `keyword` = "keyword"
    case `string` = "string"
    case `comment` = "comment"
    case `number` = "number"
    case `type` = "type"
    case `function` = "function"
    case `property` = "property"
    case `operator` = "operator"
    case `punctuation` = "punctuation"
    case `constant` = "constant"
    case `variable` = "variable"
}

public enum DiagnosticSeverity: String, Codable, Sendable {
    case `error` = "error"
    case `warning` = "warning"
    case `information` = "information"
    case `hint` = "hint"
}

public enum FileSymbolKind: String, Codable, Sendable {
    case `function` = "function"
    case `type` = "type"
    case `method` = "method"
    case `property` = "property"
    case `variable` = "variable"
    case `module` = "module"
}

public enum DiffLineKind: String, Codable, Sendable {
    case `context` = "context"
    case `added` = "added"
    case `removed` = "removed"
}

public enum LspState: String, Codable, Sendable {
    case `starting` = "starting"
    case `ready` = "ready"
    case `restarting` = "restarting"
    case `unavailable` = "unavailable"
}

public struct TextPosition: Codable, Sendable {
    public var `line`: UInt32
    public var `column`: UInt32
    public init(`line`: UInt32, `column`: UInt32) {
        self.`line` = `line`
        self.`column` = `column`
    }
}

public struct TextRange: Codable, Sendable {
    public var `start`: TextPosition
    public var `end`: TextPosition
    public init(`start`: TextPosition, `end`: TextPosition) {
        self.`start` = `start`
        self.`end` = `end`
    }
}

public struct TextEdit: Codable, Sendable {
    public var `range`: TextRange
    public var `text`: String
    public init(`range`: TextRange, `text`: String) {
        self.`range` = `range`
        self.`text` = `text`
    }
}

public struct HighlightSpan: Codable, Sendable {
    public var `startColumn`: UInt32
    public var `endColumn`: UInt32
    public var `kind`: HighlightKind
    public init(`startColumn`: UInt32, `endColumn`: UInt32, `kind`: HighlightKind) {
        self.`startColumn` = `startColumn`
        self.`endColumn` = `endColumn`
        self.`kind` = `kind`
    }
}

public struct EditorLine: Codable, Sendable {
    public var `line`: UInt32
    public var `text`: String
    public var `highlights`: [HighlightSpan]
    public init(`line`: UInt32, `text`: String, `highlights`: [HighlightSpan]) {
        self.`line` = `line`
        self.`text` = `text`
        self.`highlights` = `highlights`
    }
}

public struct FileAuthor: Codable, Sendable {
    public var `actor`: Actor
    public var `name`: String
    public init(`actor`: Actor, `name`: String) {
        self.`actor` = `actor`
        self.`name` = `name`
    }
}

public struct FileDocument: Codable, Sendable {
    public var `fileId`: Id
    public var `paneId`: Id
    public var `workspaceId`: Id
    public var `path`: String
    public var `language`: EditorLanguage
    public var `revision`: Decimal
    public var `diskRevision`: Decimal
    public var `lineCount`: UInt32
    public var `byteCount`: Decimal
    public var `dirty`: Bool
    public var `conflicted`: Bool
    public var `watching`: Bool
    public init(`fileId`: Id, `paneId`: Id, `workspaceId`: Id, `path`: String, `language`: EditorLanguage, `revision`: Decimal, `diskRevision`: Decimal, `lineCount`: UInt32, `byteCount`: Decimal, `dirty`: Bool, `conflicted`: Bool, `watching`: Bool) {
        self.`fileId` = `fileId`
        self.`paneId` = `paneId`
        self.`workspaceId` = `workspaceId`
        self.`path` = `path`
        self.`language` = `language`
        self.`revision` = `revision`
        self.`diskRevision` = `diskRevision`
        self.`lineCount` = `lineCount`
        self.`byteCount` = `byteCount`
        self.`dirty` = `dirty`
        self.`conflicted` = `conflicted`
        self.`watching` = `watching`
    }
}

public struct FileOpenParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `path`: String
    public var `line`: UInt32?
    public var `column`: UInt32?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `path`: String, `line`: UInt32? = nil, `column`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`path` = `path`
        self.`line` = `line`
        self.`column` = `column`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileOpenResult: Codable, Sendable {
    public var `file`: FileDocument
    public var `pane`: Pane
    public var `viewport`: FileReadResult
    public init(`file`: FileDocument, `pane`: Pane, `viewport`: FileReadResult) {
        self.`file` = `file`
        self.`pane` = `pane`
        self.`viewport` = `viewport`
    }
}

public struct FileReadParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal?
    public var `source`: EditorContentSource?
    public var `versionId`: Id?
    public var `fromLine`: UInt32
    public var `lineCount`: UInt32
    public init(`fileId`: Id, `revision`: Decimal? = nil, `source`: EditorContentSource? = nil, `versionId`: Id? = nil, `fromLine`: UInt32, `lineCount`: UInt32) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`source` = `source`
        self.`versionId` = `versionId`
        self.`fromLine` = `fromLine`
        self.`lineCount` = `lineCount`
    }
}

public struct FileReadResult: Codable, Sendable {
    public var `file`: FileDocument
    public var `source`: EditorContentSource
    public var `fromLine`: UInt32
    public var `toLine`: UInt32
    public var `lines`: [EditorLine]
    public init(`file`: FileDocument, `source`: EditorContentSource, `fromLine`: UInt32, `toLine`: UInt32, `lines`: [EditorLine]) {
        self.`file` = `file`
        self.`source` = `source`
        self.`fromLine` = `fromLine`
        self.`toLine` = `toLine`
        self.`lines` = `lines`
    }
}

public struct FileWriteParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `edits`: [TextEdit]
    public var `author`: FileAuthor
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `edits`: [TextEdit], `author`: FileAuthor, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`edits` = `edits`
        self.`author` = `author`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileWriteResult: Codable, Sendable {
    public var `file`: FileDocument
    public var `changedLines`: [UInt32]
    public init(`file`: FileDocument, `changedLines`: [UInt32]) {
        self.`file` = `file`
        self.`changedLines` = `changedLines`
    }
}

public struct FileWatchParams: Codable, Sendable {
    public var `fileId`: Id
    public var `watch`: Bool
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `watch`: Bool, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`watch` = `watch`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileWatchResult: Codable, Sendable {
    public var `fileId`: Id
    public var `watching`: Bool
    public init(`fileId`: Id, `watching`: Bool) {
        self.`fileId` = `fileId`
        self.`watching` = `watching`
    }
}

public struct FileVersion: Codable, Sendable {
    public var `versionId`: Id
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `createdAt`: String
    public var `author`: FileAuthor
    public var `byteCount`: Decimal
    public init(`versionId`: Id, `fileId`: Id, `revision`: Decimal, `createdAt`: String, `author`: FileAuthor, `byteCount`: Decimal) {
        self.`versionId` = `versionId`
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`createdAt` = `createdAt`
        self.`author` = `author`
        self.`byteCount` = `byteCount`
    }
}

public struct FileHistoryParams: Codable, Sendable {
    public var `fileId`: Id
    public var `cursor`: Id?
    public var `limit`: UInt32
    public init(`fileId`: Id, `cursor`: Id? = nil, `limit`: UInt32) {
        self.`fileId` = `fileId`
        self.`cursor` = `cursor`
        self.`limit` = `limit`
    }
}

public struct FileHistoryResult: Codable, Sendable {
    public var `items`: [FileVersion]
    public var `nextCursor`: Id?
    public init(`items`: [FileVersion], `nextCursor`: Id? = nil) {
        self.`items` = `items`
        self.`nextCursor` = `nextCursor`
    }
}

public struct FileRevertParams: Codable, Sendable {
    public var `fileId`: Id
    public var `versionId`: Id
    public var `baseRevision`: Decimal
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `versionId`: Id, `baseRevision`: Decimal, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`versionId` = `versionId`
        self.`baseRevision` = `baseRevision`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileRevertResult: Codable, Sendable {
    public var `file`: FileDocument
    public init(`file`: FileDocument) {
        self.`file` = `file`
    }
}

public struct FileSearchMatch: Codable, Sendable {
    public var `workspaceId`: Id
    public var `path`: String
    public var `range`: TextRange
    public var `preview`: String
    public init(`workspaceId`: Id, `path`: String, `range`: TextRange, `preview`: String) {
        self.`workspaceId` = `workspaceId`
        self.`path` = `path`
        self.`range` = `range`
        self.`preview` = `preview`
    }
}

public struct FileSearchParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `scope`: FileSearchScope?
    public var `query`: String
    public var `replacement`: String?
    public var `caseSensitive`: Bool
    public var `limit`: UInt32
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `scope`: FileSearchScope? = nil, `query`: String, `replacement`: String? = nil, `caseSensitive`: Bool, `limit`: UInt32, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`scope` = `scope`
        self.`query` = `query`
        self.`replacement` = `replacement`
        self.`caseSensitive` = `caseSensitive`
        self.`limit` = `limit`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileSearchResult: Codable, Sendable {
    public var `items`: [FileSearchMatch]
    public var `replaced`: UInt32
    public init(`items`: [FileSearchMatch], `replaced`: UInt32) {
        self.`items` = `items`
        self.`replaced` = `replaced`
    }
}

public struct FileSymbol: Codable, Sendable {
    public var `name`: String
    public var `kind`: FileSymbolKind
    public var `range`: TextRange
    public var `selectionRange`: TextRange
    public init(`name`: String, `kind`: FileSymbolKind, `range`: TextRange, `selectionRange`: TextRange) {
        self.`name` = `name`
        self.`kind` = `kind`
        self.`range` = `range`
        self.`selectionRange` = `selectionRange`
    }
}

public struct FileSymbolsParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public init(`fileId`: Id, `revision`: Decimal) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
    }
}

public struct FileSymbolsResult: Codable, Sendable {
    public var `items`: [FileSymbol]
    public init(`items`: [FileSymbol]) {
        self.`items` = `items`
    }
}

public struct FileLocation: Codable, Sendable {
    public var `workspaceId`: Id
    public var `path`: String
    public var `range`: TextRange
    public init(`workspaceId`: Id, `path`: String, `range`: TextRange) {
        self.`workspaceId` = `workspaceId`
        self.`path` = `path`
        self.`range` = `range`
    }
}

public struct FilePositionParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `position`: TextPosition
    public init(`fileId`: Id, `revision`: Decimal, `position`: TextPosition) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`position` = `position`
    }
}

public struct FileDefinitionResult: Codable, Sendable {
    public var `location`: FileLocation?
    public init(`location`: FileLocation? = nil) {
        self.`location` = `location`
    }
}

public struct FileReferencesResult: Codable, Sendable {
    public var `items`: [FileLocation]
    public init(`items`: [FileLocation]) {
        self.`items` = `items`
    }
}

public struct FileDiagnostic: Codable, Sendable {
    public var `diagnosticId`: Id
    public var `range`: TextRange
    public var `severity`: DiagnosticSeverity
    public var `message`: String
    public var `source`: String
    public init(`diagnosticId`: Id, `range`: TextRange, `severity`: DiagnosticSeverity, `message`: String, `source`: String) {
        self.`diagnosticId` = `diagnosticId`
        self.`range` = `range`
        self.`severity` = `severity`
        self.`message` = `message`
        self.`source` = `source`
    }
}

public struct FileDiagnosticsParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public init(`fileId`: Id, `revision`: Decimal) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
    }
}

public struct FileDiagnosticsResult: Codable, Sendable {
    public var `items`: [FileDiagnostic]
    public init(`items`: [FileDiagnostic]) {
        self.`items` = `items`
    }
}

public struct FileBlame: Codable, Sendable {
    public var `line`: UInt32
    public var `commit`: String
    public var `author`: String
    public var `summary`: String
    public init(`line`: UInt32, `commit`: String, `author`: String, `summary`: String) {
        self.`line` = `line`
        self.`commit` = `commit`
        self.`author` = `author`
        self.`summary` = `summary`
    }
}

public struct DiffLine: Codable, Sendable {
    public var `kind`: DiffLineKind
    public var `oldLine`: UInt32?
    public var `newLine`: UInt32?
    public var `text`: String
    public var `author`: FileAuthor?
    public init(`kind`: DiffLineKind, `oldLine`: UInt32? = nil, `newLine`: UInt32? = nil, `text`: String, `author`: FileAuthor? = nil) {
        self.`kind` = `kind`
        self.`oldLine` = `oldLine`
        self.`newLine` = `newLine`
        self.`text` = `text`
        self.`author` = `author`
    }
}

public struct DiffHunk: Codable, Sendable {
    public var `hunkId`: Id
    public var `oldStart`: UInt32
    public var `oldLines`: UInt32
    public var `newStart`: UInt32
    public var `newLines`: UInt32
    public var `lines`: [DiffLine]
    public init(`hunkId`: Id, `oldStart`: UInt32, `oldLines`: UInt32, `newStart`: UInt32, `newLines`: UInt32, `lines`: [DiffLine]) {
        self.`hunkId` = `hunkId`
        self.`oldStart` = `oldStart`
        self.`oldLines` = `oldLines`
        self.`newStart` = `newStart`
        self.`newLines` = `newLines`
        self.`lines` = `lines`
    }
}

public struct FileDiffParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public init(`fileId`: Id, `revision`: Decimal) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
    }
}

public struct FileDiffResult: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `hunks`: [DiffHunk]
    public var `blame`: [FileBlame]
    public init(`fileId`: Id, `revision`: Decimal, `hunks`: [DiffHunk], `blame`: [FileBlame]) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`hunks` = `hunks`
        self.`blame` = `blame`
    }
}

public struct FileFormatParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct FileFormatResult: Codable, Sendable {
    public var `file`: FileDocument
    public init(`file`: FileDocument) {
        self.`file` = `file`
    }
}

public struct EditorReviewParams: Codable, Sendable {
    public var `fileId`: Id
    public var `hunkId`: Id
    public var `baseRevision`: Decimal
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `hunkId`: Id, `baseRevision`: Decimal, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`hunkId` = `hunkId`
        self.`baseRevision` = `baseRevision`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct EditorReviewResult: Codable, Sendable {
    public var `file`: FileDocument
    public var `hunkId`: Id
    public init(`file`: FileDocument, `hunkId`: Id) {
        self.`file` = `file`
        self.`hunkId` = `hunkId`
    }
}

public struct EditorReviewCommentParams: Codable, Sendable {
    public var `fileId`: Id
    public var `range`: TextRange
    public var `body`: String
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `range`: TextRange, `body`: String, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`range` = `range`
        self.`body` = `body`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct EditorReviewCommentResult: Codable, Sendable {
    public var `commentId`: Id
    public var `taskId`: Id?
    public var `delivered`: Bool
    public init(`commentId`: Id, `taskId`: Id? = nil, `delivered`: Bool) {
        self.`commentId` = `commentId`
        self.`taskId` = `taskId`
        self.`delivered` = `delivered`
    }
}

public struct LspServerStatus: Codable, Sendable {
    public var `language`: EditorLanguage
    public var `state`: LspState
    public var `generation`: UInt32
    public var `detail`: String?
    public init(`language`: EditorLanguage, `state`: LspState, `generation`: UInt32, `detail`: String? = nil) {
        self.`language` = `language`
        self.`state` = `state`
        self.`generation` = `generation`
        self.`detail` = `detail`
    }
}

public struct LspStatusParams: Codable, Sendable {
    public var `workspaceId`: Id?
    public var `language`: EditorLanguage?
    public init(`workspaceId`: Id? = nil, `language`: EditorLanguage? = nil) {
        self.`workspaceId` = `workspaceId`
        self.`language` = `language`
    }
}

public struct LspStatusResult: Codable, Sendable {
    public var `servers`: [LspServerStatus]
    public init(`servers`: [LspServerStatus]) {
        self.`servers` = `servers`
    }
}

public struct FileChangedOnDiskEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `diskRevision`: Decimal
    public var `changedLines`: [UInt32]
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `diskRevision`: Decimal, `changedLines`: [UInt32]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`diskRevision` = `diskRevision`
        self.`changedLines` = `changedLines`
    }
}

public struct FileSavedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `file`: FileDocument
    public var `by`: FileAuthor
    public init(`eventId`: Decimal, `at`: String, `file`: FileDocument, `by`: FileAuthor) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`file` = `file`
        self.`by` = `by`
    }
}

public struct FileConflictEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `draftRevision`: Decimal
    public var `diskRevision`: Decimal
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `baseRevision`: Decimal, `draftRevision`: Decimal, `diskRevision`: Decimal) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`draftRevision` = `draftRevision`
        self.`diskRevision` = `diskRevision`
    }
}

public struct LspDiagnosticsEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `items`: [FileDiagnostic]
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `revision`: Decimal, `items`: [FileDiagnostic]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`items` = `items`
    }
}

public struct EditorAgentCursorEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `paneId`: Id
    public var `name`: String
    public var `position`: TextPosition
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `paneId`: Id, `name`: String, `position`: TextPosition) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`paneId` = `paneId`
        self.`name` = `name`
        self.`position` = `position`
    }
}

public struct EditorReviewRequestedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `paneId`: Id
    public var `hunks`: [DiffHunk]
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `revision`: Decimal, `paneId`: Id, `hunks`: [DiffHunk]) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`paneId` = `paneId`
        self.`hunks` = `hunks`
    }
}

public enum ClientCapability: String, Codable, Sendable {
    case `web_host` = "web_host"
}

public enum WebInteractionMode: String, Codable, Sendable {
    case `structured` = "structured"
    case `human` = "human"
}

public enum WebNavigationAction: String, Codable, Sendable {
    case `back` = "back"
    case `forward` = "forward"
    case `reload` = "reload"
    case `reload_ignoring_cache` = "reload_ignoring_cache"
}

public enum WebViewportPreset: String, Codable, Sendable {
    case `iphone_se` = "iphone_se"
    case `iphone15_pro` = "iphone_15_pro"
    case `ipad` = "ipad"
    case `responsive` = "responsive"
}

public enum WebConsoleLevel: String, Codable, Sendable {
    case `debug` = "debug"
    case `info` = "info"
    case `warning` = "warning"
    case `error` = "error"
}

public enum WebSessionState: String, Codable, Sendable {
    case `anonymous` = "anonymous"
    case `permission_required` = "permission_required"
    case `granted` = "granted"
    case `incognito` = "incognito"
}

public enum WebCommandKind: String, Codable, Sendable {
    case `navigate` = "navigate"
    case `snapshot` = "snapshot"
    case `click` = "click"
    case `type` = "type"
    case `press` = "press"
    case `hover` = "hover"
    case `select` = "select"
    case `scroll` = "scroll"
    case `wait` = "wait"
    case `evaluate` = "evaluate"
    case `screenshot` = "screenshot"
    case `console` = "console"
    case `network` = "network"
    case `resize` = "resize"
    case `close` = "close"
}

public struct WebViewport: Codable, Sendable {
    public var `width`: UInt32
    public var `height`: UInt32
    public var `preset`: WebViewportPreset
    public var `zoomPercent`: UInt32
    public init(`width`: UInt32, `height`: UInt32, `preset`: WebViewportPreset, `zoomPercent`: UInt32) {
        self.`width` = `width`
        self.`height` = `height`
        self.`preset` = `preset`
        self.`zoomPercent` = `zoomPercent`
    }
}

public struct WebRect: Codable, Sendable {
    public var `x`: Int64
    public var `y`: Int64
    public var `width`: UInt32
    public var `height`: UInt32
    public init(`x`: Int64, `y`: Int64, `width`: UInt32, `height`: UInt32) {
        self.`x` = `x`
        self.`y` = `y`
        self.`width` = `width`
        self.`height` = `height`
    }
}

public struct WebSnapshotNode: Codable, Sendable {
    public var `ref`: Id
    public var `role`: String
    public var `name`: String
    public var `state`: String
    public var `value`: String?
    public var `rect`: WebRect
    public var `parentRef`: Id?
    public init(`ref`: Id, `role`: String, `name`: String, `state`: String, `value`: String? = nil, `rect`: WebRect, `parentRef`: Id? = nil) {
        self.`ref` = `ref`
        self.`role` = `role`
        self.`name` = `name`
        self.`state` = `state`
        self.`value` = `value`
        self.`rect` = `rect`
        self.`parentRef` = `parentRef`
    }
}

public struct WebConsoleEntry: Codable, Sendable {
    public var `sequence`: Decimal
    public var `level`: WebConsoleLevel
    public var `text`: String
    public var `source`: String
    public var `at`: String
    public init(`sequence`: Decimal, `level`: WebConsoleLevel, `text`: String, `source`: String, `at`: String) {
        self.`sequence` = `sequence`
        self.`level` = `level`
        self.`text` = `text`
        self.`source` = `source`
        self.`at` = `at`
    }
}

public struct WebNetworkEntry: Codable, Sendable {
    public var `sequence`: Decimal
    public var `method`: String
    public var `url`: String
    public var `status`: UInt32
    public var `durationMs`: UInt32
    public var `bytes`: Decimal
    public var `at`: String
    public init(`sequence`: Decimal, `method`: String, `url`: String, `status`: UInt32, `durationMs`: UInt32, `bytes`: Decimal, `at`: String) {
        self.`sequence` = `sequence`
        self.`method` = `method`
        self.`url` = `url`
        self.`status` = `status`
        self.`durationMs` = `durationMs`
        self.`bytes` = `bytes`
        self.`at` = `at`
    }
}

public struct LocalServer: Codable, Sendable {
    public var `port`: UInt32
    public var `pid`: UInt32
    public var `command`: String
    public var `cwd`: String
    public var `title`: String?
    public init(`port`: UInt32, `pid`: UInt32, `command`: String, `cwd`: String, `title`: String? = nil) {
        self.`port` = `port`
        self.`pid` = `pid`
        self.`command` = `command`
        self.`cwd` = `cwd`
        self.`title` = `title`
    }
}

public struct WebComputedStyles: Codable, Sendable {
    public var `boxSizing`: String
    public var `color`: String
    public var `background`: String
    public var `font`: String
    public var `border`: String
    public var `borderRadius`: String
    public var `display`: String
    public var `position`: String
    public init(`boxSizing`: String, `color`: String, `background`: String, `font`: String, `border`: String, `borderRadius`: String, `display`: String, `position`: String) {
        self.`boxSizing` = `boxSizing`
        self.`color` = `color`
        self.`background` = `background`
        self.`font` = `font`
        self.`border` = `border`
        self.`borderRadius` = `borderRadius`
        self.`display` = `display`
        self.`position` = `position`
    }
}

public struct WebAccessibility: Codable, Sendable {
    public var `role`: String
    public var `name`: String
    public var `state`: String
    public init(`role`: String, `name`: String, `state`: String) {
        self.`role` = `role`
        self.`name` = `name`
        self.`state` = `state`
    }
}

public struct WebComponentSource: Codable, Sendable {
    public var `framework`: String
    public var `component`: String
    public var `sourceFile`: String?
    public var `line`: UInt32?
    public init(`framework`: String, `component`: String, `sourceFile`: String? = nil, `line`: UInt32? = nil) {
        self.`framework` = `framework`
        self.`component` = `component`
        self.`sourceFile` = `sourceFile`
        self.`line` = `line`
    }
}

public struct WebPickedElement: Codable, Sendable {
    public var `selector`: String
    public var `path`: [String]
    public var `outerHtml`: String
    public var `styles`: WebComputedStyles
    public var `accessibility`: WebAccessibility
    public var `viewportRect`: WebRect
    public var `pageRect`: WebRect
    public var `image`: AttachmentRef
    public var `component`: WebComponentSource?
    public init(`selector`: String, `path`: [String], `outerHtml`: String, `styles`: WebComputedStyles, `accessibility`: WebAccessibility, `viewportRect`: WebRect, `pageRect`: WebRect, `image`: AttachmentRef, `component`: WebComponentSource? = nil) {
        self.`selector` = `selector`
        self.`path` = `path`
        self.`outerHtml` = `outerHtml`
        self.`styles` = `styles`
        self.`accessibility` = `accessibility`
        self.`viewportRect` = `viewportRect`
        self.`pageRect` = `pageRect`
        self.`image` = `image`
        self.`component` = `component`
    }
}

public struct WebElementPackage: Codable, Sendable {
    public var `url`: String
    public var `title`: String
    public var `elements`: [WebPickedElement]
    public init(`url`: String, `title`: String, `elements`: [WebPickedElement]) {
        self.`url` = `url`
        self.`title` = `title`
        self.`elements` = `elements`
    }
}

public struct WebOpenParams: Codable, Sendable {
    public var `url`: String
    public var `workspaceId`: Id?
    public var `newPane`: Bool?
    public var `incognito`: Bool?
    public var `mode`: WebInteractionMode?
    public var `idempotencyKey`: Id
    public init(`url`: String, `workspaceId`: Id? = nil, `newPane`: Bool? = nil, `incognito`: Bool? = nil, `mode`: WebInteractionMode? = nil, `idempotencyKey`: Id) {
        self.`url` = `url`
        self.`workspaceId` = `workspaceId`
        self.`newPane` = `newPane`
        self.`incognito` = `incognito`
        self.`mode` = `mode`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebOpenResult: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct WebNavigateParams: Codable, Sendable {
    public var `paneId`: Id
    public var `url`: String?
    public var `action`: WebNavigationAction?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `url`: String? = nil, `action`: WebNavigationAction? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`url` = `url`
        self.`action` = `action`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebNavigateResult: Codable, Sendable {
    public var `paneId`: Id
    public var `accepted`: Bool
    public init(`paneId`: Id, `accepted`: Bool) {
        self.`paneId` = `paneId`
        self.`accepted` = `accepted`
    }
}

public struct WebSnapshotParams: Codable, Sendable {
    public var `paneId`: Id
    public var `screenshot`: Bool?
    public init(`paneId`: Id, `screenshot`: Bool? = nil) {
        self.`paneId` = `paneId`
        self.`screenshot` = `screenshot`
    }
}

public struct WebSnapshotResult: Codable, Sendable {
    public var `paneId`: Id
    public var `navigationGeneration`: Decimal
    public var `title`: String
    public var `url`: String
    public var `nodes`: [WebSnapshotNode]
    public var `truncated`: Bool
    public var `screenshot`: AttachmentRef?
    public init(`paneId`: Id, `navigationGeneration`: Decimal, `title`: String, `url`: String, `nodes`: [WebSnapshotNode], `truncated`: Bool, `screenshot`: AttachmentRef? = nil) {
        self.`paneId` = `paneId`
        self.`navigationGeneration` = `navigationGeneration`
        self.`title` = `title`
        self.`url` = `url`
        self.`nodes` = `nodes`
        self.`truncated` = `truncated`
        self.`screenshot` = `screenshot`
    }
}

public struct WebActionResult: Codable, Sendable {
    public var `paneId`: Id
    public var `completed`: Bool
    public init(`paneId`: Id, `completed`: Bool) {
        self.`paneId` = `paneId`
        self.`completed` = `completed`
    }
}

public struct WebClickParams: Codable, Sendable {
    public var `paneId`: Id
    public var `ref`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `ref`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`ref` = `ref`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebTypeParams: Codable, Sendable {
    public var `paneId`: Id
    public var `ref`: Id
    public var `text`: String
    public var `submit`: Bool?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `ref`: Id, `text`: String, `submit`: Bool? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`ref` = `ref`
        self.`text` = `text`
        self.`submit` = `submit`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebPressParams: Codable, Sendable {
    public var `paneId`: Id
    public var `key`: String
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `key`: String, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`key` = `key`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebHoverParams: Codable, Sendable {
    public var `paneId`: Id
    public var `ref`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `ref`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`ref` = `ref`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebSelectParams: Codable, Sendable {
    public var `paneId`: Id
    public var `ref`: Id
    public var `value`: String
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `ref`: Id, `value`: String, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`ref` = `ref`
        self.`value` = `value`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebScrollParams: Codable, Sendable {
    public var `paneId`: Id
    public var `ref`: Id?
    public var `dx`: Int64
    public var `dy`: Int64
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `ref`: Id? = nil, `dx`: Int64, `dy`: Int64, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`ref` = `ref`
        self.`dx` = `dx`
        self.`dy` = `dy`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebWaitParams: Codable, Sendable {
    public var `paneId`: Id
    public var `text`: String?
    public var `selector`: String?
    public var `networkIdle`: Bool?
    public var `timeoutMs`: UInt32
    public init(`paneId`: Id, `text`: String? = nil, `selector`: String? = nil, `networkIdle`: Bool? = nil, `timeoutMs`: UInt32) {
        self.`paneId` = `paneId`
        self.`text` = `text`
        self.`selector` = `selector`
        self.`networkIdle` = `networkIdle`
        self.`timeoutMs` = `timeoutMs`
    }
}

public struct WebWaitResult: Codable, Sendable {
    public var `paneId`: Id
    public var `matched`: Bool
    public init(`paneId`: Id, `matched`: Bool) {
        self.`paneId` = `paneId`
        self.`matched` = `matched`
    }
}

public struct WebEvaluateParams: Codable, Sendable {
    public var `paneId`: Id
    public var `js`: String
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `js`: String, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`js` = `js`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebEvaluateResult: Codable, Sendable {
    public var `paneId`: Id
    public var `value`: String
    public init(`paneId`: Id, `value`: String) {
        self.`paneId` = `paneId`
        self.`value` = `value`
    }
}

public struct WebScreenshotParams: Codable, Sendable {
    public var `paneId`: Id
    public var `fullPage`: Bool?
    public init(`paneId`: Id, `fullPage`: Bool? = nil) {
        self.`paneId` = `paneId`
        self.`fullPage` = `fullPage`
    }
}

public struct WebScreenshotResult: Codable, Sendable {
    public var `paneId`: Id
    public var `image`: AttachmentRef
    public init(`paneId`: Id, `image`: AttachmentRef) {
        self.`paneId` = `paneId`
        self.`image` = `image`
    }
}

public struct WebConsoleParams: Codable, Sendable {
    public var `paneId`: Id
    public var `since`: Decimal?
    public init(`paneId`: Id, `since`: Decimal? = nil) {
        self.`paneId` = `paneId`
        self.`since` = `since`
    }
}

public struct WebConsoleResult: Codable, Sendable {
    public var `paneId`: Id
    public var `entries`: [WebConsoleEntry]
    public var `next`: Decimal
    public var `truncated`: Bool
    public init(`paneId`: Id, `entries`: [WebConsoleEntry], `next`: Decimal, `truncated`: Bool) {
        self.`paneId` = `paneId`
        self.`entries` = `entries`
        self.`next` = `next`
        self.`truncated` = `truncated`
    }
}

public struct WebNetworkParams: Codable, Sendable {
    public var `paneId`: Id
    public var `since`: Decimal?
    public init(`paneId`: Id, `since`: Decimal? = nil) {
        self.`paneId` = `paneId`
        self.`since` = `since`
    }
}

public struct WebNetworkResult: Codable, Sendable {
    public var `paneId`: Id
    public var `entries`: [WebNetworkEntry]
    public var `next`: Decimal
    public var `truncated`: Bool
    public init(`paneId`: Id, `entries`: [WebNetworkEntry], `next`: Decimal, `truncated`: Bool) {
        self.`paneId` = `paneId`
        self.`entries` = `entries`
        self.`next` = `next`
        self.`truncated` = `truncated`
    }
}

public struct WebResizeParams: Codable, Sendable {
    public var `paneId`: Id
    public var `preset`: WebViewportPreset?
    public var `width`: UInt32?
    public var `height`: UInt32?
    public var `rotate`: Bool?
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `preset`: WebViewportPreset? = nil, `width`: UInt32? = nil, `height`: UInt32? = nil, `rotate`: Bool? = nil, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`preset` = `preset`
        self.`width` = `width`
        self.`height` = `height`
        self.`rotate` = `rotate`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebResizeResult: Codable, Sendable {
    public var `paneId`: Id
    public var `viewport`: WebViewport
    public init(`paneId`: Id, `viewport`: WebViewport) {
        self.`paneId` = `paneId`
        self.`viewport` = `viewport`
    }
}

public struct WebCloseParams: Codable, Sendable {
    public var `paneId`: Id
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebCloseResult: Codable, Sendable {
    public var `paneId`: Id
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct WebCommandParams: Codable, Sendable {
    public var `paneId`: Id
    public var `command`: WebCommandKind
    public var `args`: JsonValue
    public var `timeoutMs`: UInt32
    public var `operationKey`: Id
    public init(`paneId`: Id, `command`: WebCommandKind, `args`: JsonValue, `timeoutMs`: UInt32, `operationKey`: Id) {
        self.`paneId` = `paneId`
        self.`command` = `command`
        self.`args` = `args`
        self.`timeoutMs` = `timeoutMs`
        self.`operationKey` = `operationKey`
    }
}

public struct WebCommandResult: Codable, Sendable {
    public var `paneId`: Id
    public var `value`: JsonValue?
    public var `cookiesPresent`: Bool?
    public init(`paneId`: Id, `value`: JsonValue? = nil, `cookiesPresent`: Bool? = nil) {
        self.`paneId` = `paneId`
        self.`value` = `value`
        self.`cookiesPresent` = `cookiesPresent`
    }
}

public struct ServersLocalListParams: Codable, Sendable {

    public init() {

    }
}

public struct ServersLocalListResult: Codable, Sendable {
    public var `servers`: [LocalServer]
    public init(`servers`: [LocalServer]) {
        self.`servers` = `servers`
    }
}

public struct WebHostNavigatedParams: Codable, Sendable {
    public var `paneId`: Id
    public var `navigationGeneration`: Decimal
    public var `url`: String
    public var `title`: String
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `navigationGeneration`: Decimal, `url`: String, `title`: String, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`navigationGeneration` = `navigationGeneration`
        self.`url` = `url`
        self.`title` = `title`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebHostNavigatedResult: Codable, Sendable {
    public var `accepted`: Bool
    public init(`accepted`: Bool) {
        self.`accepted` = `accepted`
    }
}

public struct WebHostConsoleCountsParams: Codable, Sendable {
    public var `paneId`: Id
    public var `warnings`: UInt32
    public var `errors`: UInt32
    public var `idempotencyKey`: Id
    public init(`paneId`: Id, `warnings`: UInt32, `errors`: UInt32, `idempotencyKey`: Id) {
        self.`paneId` = `paneId`
        self.`warnings` = `warnings`
        self.`errors` = `errors`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WebHostConsoleCountsResult: Codable, Sendable {
    public var `accepted`: Bool
    public init(`accepted`: Bool) {
        self.`accepted` = `accepted`
    }
}

public struct WebNavigatedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `paneId`: Id
    public var `navigationGeneration`: Decimal
    public var `url`: String
    public var `title`: String
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `navigationGeneration`: Decimal, `url`: String, `title`: String) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`paneId` = `paneId`
        self.`navigationGeneration` = `navigationGeneration`
        self.`url` = `url`
        self.`title` = `title`
    }
}

public struct WebConsoleCountsEvent: Codable, Sendable {
    public var `paneId`: Id
    public var `warnings`: UInt32
    public var `errors`: UInt32
    public init(`paneId`: Id, `warnings`: UInt32, `errors`: UInt32) {
        self.`paneId` = `paneId`
        self.`warnings` = `warnings`
        self.`errors` = `errors`
    }
}

public struct WebAgentDrivingEvent: Codable, Sendable {
    public var `paneId`: Id
    public var `on`: Bool
    public var `domain`: String?
    public init(`paneId`: Id, `on`: Bool, `domain`: String? = nil) {
        self.`paneId` = `paneId`
        self.`on` = `on`
        self.`domain` = `domain`
    }
}

public struct ServersLocalChangedEvent: Codable, Sendable {
    public var `servers`: [LocalServer]
    public init(`servers`: [LocalServer]) {
        self.`servers` = `servers`
    }
}

public enum PermissionRequestKind: String, Codable, Sendable {
    case `agent` = "agent"
    case `web_session` = "web_session"
    case `web_evaluate` = "web_evaluate"
}

public enum WritingKind: String, Codable, Sendable {
    case `text` = "text"
    case `presentation` = "presentation"
    case `screenplay` = "screenplay"
}

public enum WritingSection: String, Codable, Sendable {
    case `texts` = "texts"
    case `presentations` = "presentations"
    case `screenplays` = "screenplays"
}

public enum WritingFont: String, Codable, Sendable {
    case `reading_serif` = "reading_serif"
    case `classic_serif` = "classic_serif"
    case `sans` = "sans"
}

public enum WritingFormatKind: String, Codable, Sendable {
    case `heading1` = "heading_1"
    case `heading2` = "heading_2"
    case `heading3` = "heading_3"
    case `bold` = "bold"
    case `italic` = "italic"
    case `bullet_list` = "bullet_list"
    case `numbered_list` = "numbered_list"
    case `quote` = "quote"
    case `link` = "link"
    case `image` = "image"
}

public enum ScreenplayElementKind: String, Codable, Sendable {
    case `scene_heading` = "scene_heading"
    case `action` = "action"
    case `character` = "character"
    case `parenthetical` = "parenthetical"
    case `dialogue` = "dialogue"
    case `transition` = "transition"
    case `shot` = "shot"
}

public enum BeatCategory: String, Codable, Sendable {
    case `story` = "story"
    case `character` = "character"
    case `setting` = "setting"
    case `research` = "research"
}

public struct WritingDocument: Codable, Sendable {
    public var `file`: FileDocument
    public var `kind`: WritingKind
    public var `section`: WritingSection
    public var `sidecarRevision`: Decimal
    public var `font`: WritingFont
    public var `focusMode`: Bool
    public init(`file`: FileDocument, `kind`: WritingKind, `section`: WritingSection, `sidecarRevision`: Decimal, `font`: WritingFont, `focusMode`: Bool) {
        self.`file` = `file`
        self.`kind` = `kind`
        self.`section` = `section`
        self.`sidecarRevision` = `sidecarRevision`
        self.`font` = `font`
        self.`focusMode` = `focusMode`
    }
}

public struct WritingSectionEntry: Codable, Sendable {
    public var `path`: String
    public var `title`: String
    public var `kind`: WritingKind
    public var `section`: WritingSection
    public var `fileId`: Id?
    public var `modifiedAt`: String
    public var `byteCount`: Decimal
    public init(`path`: String, `title`: String, `kind`: WritingKind, `section`: WritingSection, `fileId`: Id? = nil, `modifiedAt`: String, `byteCount`: Decimal) {
        self.`path` = `path`
        self.`title` = `title`
        self.`kind` = `kind`
        self.`section` = `section`
        self.`fileId` = `fileId`
        self.`modifiedAt` = `modifiedAt`
        self.`byteCount` = `byteCount`
    }
}

public struct WritingSlide: Codable, Sendable {
    public var `index`: UInt32
    public var `startLine`: UInt32
    public var `endLine`: UInt32
    public var `heading`: String
    public var `notes`: String
    public var `imagePaths`: [String]
    public init(`index`: UInt32, `startLine`: UInt32, `endLine`: UInt32, `heading`: String, `notes`: String, `imagePaths`: [String]) {
        self.`index` = `index`
        self.`startLine` = `startLine`
        self.`endLine` = `endLine`
        self.`heading` = `heading`
        self.`notes` = `notes`
        self.`imagePaths` = `imagePaths`
    }
}

public struct ScreenplayElement: Codable, Sendable {
    public var `elementId`: Id
    public var `kind`: ScreenplayElementKind
    public var `startLine`: UInt32
    public var `endLine`: UInt32
    public var `text`: String
    public var `sceneId`: Id?
    public init(`elementId`: Id, `kind`: ScreenplayElementKind, `startLine`: UInt32, `endLine`: UInt32, `text`: String, `sceneId`: Id? = nil) {
        self.`elementId` = `elementId`
        self.`kind` = `kind`
        self.`startLine` = `startLine`
        self.`endLine` = `endLine`
        self.`text` = `text`
        self.`sceneId` = `sceneId`
    }
}

public struct ScreenplayScene: Codable, Sendable {
    public var `sceneId`: Id
    public var `ordinal`: UInt32
    public var `heading`: String
    public var `line`: UInt32
    public var `referenceImage`: String?
    public var `caption`: String?
    public var `promptIdea`: String?
    public var `linkedBeatIds`: [Id]
    public init(`sceneId`: Id, `ordinal`: UInt32, `heading`: String, `line`: UInt32, `referenceImage`: String? = nil, `caption`: String? = nil, `promptIdea`: String? = nil, `linkedBeatIds`: [Id]) {
        self.`sceneId` = `sceneId`
        self.`ordinal` = `ordinal`
        self.`heading` = `heading`
        self.`line` = `line`
        self.`referenceImage` = `referenceImage`
        self.`caption` = `caption`
        self.`promptIdea` = `promptIdea`
        self.`linkedBeatIds` = `linkedBeatIds`
    }
}

public struct WritingVariant: Codable, Sendable {
    public var `variantId`: Id
    public var `text`: String
    public var `author`: FileAuthor
    public init(`variantId`: Id, `text`: String, `author`: FileAuthor) {
        self.`variantId` = `variantId`
        self.`text` = `text`
        self.`author` = `author`
    }
}

public struct WritingLineVariants: Codable, Sendable {
    public var `lineId`: Id
    public var `line`: UInt32
    public var `alternates`: [WritingVariant]
    public var `selectedIndex`: UInt32
    public init(`lineId`: Id, `line`: UInt32, `alternates`: [WritingVariant], `selectedIndex`: UInt32) {
        self.`lineId` = `lineId`
        self.`line` = `line`
        self.`alternates` = `alternates`
        self.`selectedIndex` = `selectedIndex`
    }
}

public struct BeatCard: Codable, Sendable {
    public var `beatId`: Id
    public var `title`: String
    public var `body`: String
    public var `category`: BeatCategory
    public var `order`: UInt32
    public var `sceneId`: Id?
    public init(`beatId`: Id, `title`: String, `body`: String, `category`: BeatCategory, `order`: UInt32, `sceneId`: Id? = nil) {
        self.`beatId` = `beatId`
        self.`title` = `title`
        self.`body` = `body`
        self.`category` = `category`
        self.`order` = `order`
        self.`sceneId` = `sceneId`
    }
}

public struct WritingSectionListParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `section`: WritingSection
    public var `cursor`: Id?
    public var `limit`: UInt32
    public init(`workspaceId`: Id, `section`: WritingSection, `cursor`: Id? = nil, `limit`: UInt32) {
        self.`workspaceId` = `workspaceId`
        self.`section` = `section`
        self.`cursor` = `cursor`
        self.`limit` = `limit`
    }
}

public struct WritingSectionListResult: Codable, Sendable {
    public var `entries`: [WritingSectionEntry]
    public var `nextCursor`: Id?
    public init(`entries`: [WritingSectionEntry], `nextCursor`: Id? = nil) {
        self.`entries` = `entries`
        self.`nextCursor` = `nextCursor`
    }
}

public struct WritingDocumentCreateParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `section`: WritingSection
    public var `title`: String
    public var `relativeFolder`: String?
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `section`: WritingSection, `title`: String, `relativeFolder`: String? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`section` = `section`
        self.`title` = `title`
        self.`relativeFolder` = `relativeFolder`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingDocumentCreateResult: Codable, Sendable {
    public var `document`: WritingDocument
    public var `pane`: Pane
    public var `viewport`: FileReadResult
    public init(`document`: WritingDocument, `pane`: Pane, `viewport`: FileReadResult) {
        self.`document` = `document`
        self.`pane` = `pane`
        self.`viewport` = `viewport`
    }
}

public struct WritingDocumentOpenParams: Codable, Sendable {
    public var `workspaceId`: Id
    public var `path`: String
    public var `kind`: WritingKind
    public var `idempotencyKey`: Id
    public init(`workspaceId`: Id, `path`: String, `kind`: WritingKind, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`path` = `path`
        self.`kind` = `kind`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingDocumentOpenResult: Codable, Sendable {
    public var `document`: WritingDocument
    public var `pane`: Pane
    public var `viewport`: FileReadResult
    public init(`document`: WritingDocument, `pane`: Pane, `viewport`: FileReadResult) {
        self.`document` = `document`
        self.`pane` = `pane`
        self.`viewport` = `viewport`
    }
}

public struct WritingStructureParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public init(`fileId`: Id, `revision`: Decimal) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
    }
}

public struct WritingStructureResult: Codable, Sendable {
    public var `document`: WritingDocument
    public var `slides`: [WritingSlide]
    public var `scenes`: [ScreenplayScene]
    public var `elements`: [ScreenplayElement]
    public var `variants`: [WritingLineVariants]
    public var `beats`: [BeatCard]
    public var `truncated`: Bool
    public init(`document`: WritingDocument, `slides`: [WritingSlide], `scenes`: [ScreenplayScene], `elements`: [ScreenplayElement], `variants`: [WritingLineVariants], `beats`: [BeatCard], `truncated`: Bool) {
        self.`document` = `document`
        self.`slides` = `slides`
        self.`scenes` = `scenes`
        self.`elements` = `elements`
        self.`variants` = `variants`
        self.`beats` = `beats`
        self.`truncated` = `truncated`
    }
}

public struct WritingFormatParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `range`: TextRange
    public var `kind`: WritingFormatKind
    public var `value`: String?
    public var `image`: AttachmentRef?
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `range`: TextRange, `kind`: WritingFormatKind, `value`: String? = nil, `image`: AttachmentRef? = nil, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`range` = `range`
        self.`kind` = `kind`
        self.`value` = `value`
        self.`image` = `image`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingFormatResult: Codable, Sendable {
    public var `file`: FileDocument
    public init(`file`: FileDocument) {
        self.`file` = `file`
    }
}

public struct WritingAssetReadParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `asset`: String
    public init(`fileId`: Id, `revision`: Decimal, `asset`: String) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`asset` = `asset`
    }
}

public struct WritingAssetReadResult: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public var `image`: AttachmentRef
    public init(`fileId`: Id, `revision`: Decimal, `image`: AttachmentRef) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
        self.`image` = `image`
    }
}

public struct WritingPreferencesParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `font`: WritingFont?
    public var `focusMode`: Bool?
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `font`: WritingFont? = nil, `focusMode`: Bool? = nil, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`font` = `font`
        self.`focusMode` = `focusMode`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingPreferencesResult: Codable, Sendable {
    public var `document`: WritingDocument
    public init(`document`: WritingDocument) {
        self.`document` = `document`
    }
}

public struct WritingVariantRequestParams: Codable, Sendable {
    public var `fileId`: Id
    public var `lineId`: Id
    public var `baseRevision`: Decimal
    public var `agentPaneId`: Id
    public var `instruction`: String?
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `lineId`: Id, `baseRevision`: Decimal, `agentPaneId`: Id, `instruction`: String? = nil, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`lineId` = `lineId`
        self.`baseRevision` = `baseRevision`
        self.`agentPaneId` = `agentPaneId`
        self.`instruction` = `instruction`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingVariantRequestResult: Codable, Sendable {
    public var `requestId`: Id
    public var `delivered`: Bool
    public init(`requestId`: Id, `delivered`: Bool) {
        self.`requestId` = `requestId`
        self.`delivered` = `delivered`
    }
}

public struct WritingVariantAddParams: Codable, Sendable {
    public var `fileId`: Id
    public var `lineId`: Id
    public var `baseRevision`: Decimal
    public var `text`: String
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `lineId`: Id, `baseRevision`: Decimal, `text`: String, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`lineId` = `lineId`
        self.`baseRevision` = `baseRevision`
        self.`text` = `text`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingVariantSelectParams: Codable, Sendable {
    public var `fileId`: Id
    public var `lineId`: Id
    public var `variantId`: Id
    public var `baseRevision`: Decimal
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `lineId`: Id, `variantId`: Id, `baseRevision`: Decimal, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`lineId` = `lineId`
        self.`variantId` = `variantId`
        self.`baseRevision` = `baseRevision`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingVariantRemoveParams: Codable, Sendable {
    public var `fileId`: Id
    public var `lineId`: Id
    public var `variantId`: Id
    public var `baseRevision`: Decimal
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `lineId`: Id, `variantId`: Id, `baseRevision`: Decimal, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`lineId` = `lineId`
        self.`variantId` = `variantId`
        self.`baseRevision` = `baseRevision`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingVariantResult: Codable, Sendable {
    public var `document`: WritingDocument
    public var `variants`: WritingLineVariants
    public init(`document`: WritingDocument, `variants`: WritingLineVariants) {
        self.`document` = `document`
        self.`variants` = `variants`
    }
}

public struct WritingBeatListParams: Codable, Sendable {
    public var `fileId`: Id
    public var `revision`: Decimal
    public init(`fileId`: Id, `revision`: Decimal) {
        self.`fileId` = `fileId`
        self.`revision` = `revision`
    }
}

public struct WritingBeatListResult: Codable, Sendable {
    public var `beats`: [BeatCard]
    public init(`beats`: [BeatCard]) {
        self.`beats` = `beats`
    }
}

public struct WritingBeatPutParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `beatId`: Id?
    public var `title`: String
    public var `body`: String
    public var `category`: BeatCategory
    public var `order`: UInt32
    public var `sceneId`: Id?
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `beatId`: Id? = nil, `title`: String, `body`: String, `category`: BeatCategory, `order`: UInt32, `sceneId`: Id? = nil, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`beatId` = `beatId`
        self.`title` = `title`
        self.`body` = `body`
        self.`category` = `category`
        self.`order` = `order`
        self.`sceneId` = `sceneId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingBeatDeleteParams: Codable, Sendable {
    public var `fileId`: Id
    public var `baseRevision`: Decimal
    public var `beatId`: Id
    public var `idempotencyKey`: Id
    public init(`fileId`: Id, `baseRevision`: Decimal, `beatId`: Id, `idempotencyKey`: Id) {
        self.`fileId` = `fileId`
        self.`baseRevision` = `baseRevision`
        self.`beatId` = `beatId`
        self.`idempotencyKey` = `idempotencyKey`
    }
}

public struct WritingBeatResult: Codable, Sendable {
    public var `document`: WritingDocument
    public var `beat`: BeatCard?
    public init(`document`: WritingDocument, `beat`: BeatCard? = nil) {
        self.`document` = `document`
        self.`beat` = `beat`
    }
}

public struct WritingChangedEvent: Codable, Sendable {
    public var `eventId`: Decimal
    public var `at`: String
    public var `fileId`: Id
    public var `paneId`: Id
    public var `revision`: Decimal
    public var `sidecarRevision`: Decimal
    public init(`eventId`: Decimal, `at`: String, `fileId`: Id, `paneId`: Id, `revision`: Decimal, `sidecarRevision`: Decimal) {
        self.`eventId` = `eventId`
        self.`at` = `at`
        self.`fileId` = `fileId`
        self.`paneId` = `paneId`
        self.`revision` = `revision`
        self.`sidecarRevision` = `sidecarRevision`
    }
}

public enum HarnessSource: String, Codable, Sendable {
    case `built_in` = "built_in"
    case `custom` = "custom"
}

public enum HarnessSetupState: String, Codable, Sendable {
    case `ready` = "ready"
    case `not_installed` = "not_installed"
    case `sign_in` = "sign_in"
    case `check_setup` = "check_setup"
}

public enum HarnessEnvironmentIssue: String, Codable, Sendable {
    case `missing_shell` = "missing_shell"
    case `unsupported_shell` = "unsupported_shell"
    case `unavailable_shell` = "unavailable_shell"
    case `probe_failed` = "probe_failed"
    case `probe_timed_out` = "probe_timed_out"
    case `output_too_large` = "output_too_large"
    case `invalid_output` = "invalid_output"
}

public struct HarnessEnvironmentVariable: Codable, Sendable {
    public var `key`: String
    public var `value`: String
    public init(`key`: String, `value`: String) {
        self.`key` = `key`
        self.`value` = `value`
    }
}

public enum EventName: String, Codable, Sendable, CaseIterable {
    case authChallenge = "auth.challenge"
    case stateSnapshotPart = "state.snapshot_part"
    case workspaceCreated = "workspace.created"
    case workspaceRenamed = "workspace.renamed"
    case workspaceClosed = "workspace.closed"
    case paneCreated = "pane.created"
    case paneRenamed = "pane.renamed"
    case paneClosed = "pane.closed"
    case paneStateChanged = "pane.state_changed"
    case streamReplayStart = "stream.replay_start"
    case streamReplayDone = "stream.replay_done"
    case streamGap = "stream.gap"
    case devicePairRequested = "device.pair_requested"
    case devicePaired = "device.paired"
    case deviceRevoked = "device.revoked"
    case pairResult = "pair.result"
    case engineUpdating = "engine.updating"
    case engineRestarted = "engine.restarted"
    case ptydReconnected = "ptyd.reconnected"
    case ptydLost = "ptyd.lost"
    case paneUpdated = "pane.updated"
    case settingsChanged = "settings.changed"
    case paneSessionStarted = "pane.session_started"
    case paneTurnEnded = "pane.turn_ended"
    case paneQueued = "pane.queued"
    case panePermissionRequested = "pane.permission_requested"
    case panePermissionAnswered = "pane.permission_answered"
    case paneElicitationRequested = "pane.elicitation_requested"
    case paneElicitationAnswered = "pane.elicitation_answered"
    case paneConfigChanging = "pane.config_changing"
    case paneConfigChanged = "pane.config_changed"
    case paneHarnessSwitchStarted = "pane.harness_switch_started"
    case paneHarnessSwitchDone = "pane.harness_switch_done"
    case paneHarnessSwitchFailed = "pane.harness_switch_failed"
    case paneConfigFailed = "pane.config_failed"
    case paneSessionLost = "pane.session_lost"
    case paneNeedsLogin = "pane.needs_login"
    case paneAuthFinished = "pane.auth_finished"
    case paneUpdate = "pane.update"
    case attentionChanged = "attention.changed"
    case paneQueuePaused = "pane.queue.paused"
    case paneQueueResumed = "pane.queue.resumed"
    case taskCreated = "task.created"
    case taskMoved = "task.moved"
    case taskClaimed = "task.claimed"
    case taskReleased = "task.released"
    case taskCommented = "task.commented"
    case taskSubitemChanged = "task.subitem_changed"
    case taskBlocked = "task.blocked"
    case taskUnblocked = "task.unblocked"
    case taskPhraseChanged = "task.phrase_changed"
    case taskUpdated = "task.updated"
    case taskAttention = "task.attention"
    case loopTick = "loop.tick"
    case loopStopped = "loop.stopped"
    case usageChanged = "usage.changed"
    case fileChangedOnDisk = "file.changed_on_disk"
    case fileSaved = "file.saved"
    case fileConflict = "file.conflict"
    case lspDiagnostics = "lsp.diagnostics"
    case editorAgentCursor = "editor.agent_cursor"
    case editorReviewRequested = "editor.review_requested"
    case serversLocalChanged = "servers.local.changed"
    case webNavigated = "web.navigated"
    case webConsoleCounts = "web.console_counts"
    case webAgentDriving = "web.agent_driving"
    case writingChanged = "writing.changed"
}

public struct EventDescriptor: Sendable {
    public let paramsType: String
    public let durable: Bool
    public init(paramsType: String, durable: Bool) {
        self.paramsType = paramsType
        self.durable = durable
    }
}

public let eventRegistry: [EventName: EventDescriptor] = [
    .authChallenge: EventDescriptor(paramsType: "AuthChallengeEvent", durable: false),
    .stateSnapshotPart: EventDescriptor(paramsType: "StateSnapshotPartEvent", durable: false),
    .workspaceCreated: EventDescriptor(paramsType: "WorkspaceCreatedEvent", durable: true),
    .workspaceRenamed: EventDescriptor(paramsType: "WorkspaceRenamedEvent", durable: true),
    .workspaceClosed: EventDescriptor(paramsType: "WorkspaceClosedEvent", durable: true),
    .paneCreated: EventDescriptor(paramsType: "PaneCreatedEvent", durable: true),
    .paneRenamed: EventDescriptor(paramsType: "PaneRenamedEvent", durable: true),
    .paneClosed: EventDescriptor(paramsType: "PaneClosedEvent", durable: true),
    .paneStateChanged: EventDescriptor(paramsType: "PaneStateChangedEvent", durable: true),
    .streamReplayStart: EventDescriptor(paramsType: "StreamReplayStartEvent", durable: false),
    .streamReplayDone: EventDescriptor(paramsType: "StreamReplayDoneEvent", durable: false),
    .streamGap: EventDescriptor(paramsType: "StreamGapEvent", durable: false),
    .devicePairRequested: EventDescriptor(paramsType: "DevicePairRequestedEvent", durable: true),
    .devicePaired: EventDescriptor(paramsType: "DevicePairedEvent", durable: true),
    .deviceRevoked: EventDescriptor(paramsType: "DeviceRevokedEvent", durable: true),
    .pairResult: EventDescriptor(paramsType: "PairResultEvent", durable: false),
    .engineUpdating: EventDescriptor(paramsType: "EngineUpdatingEvent", durable: true),
    .engineRestarted: EventDescriptor(paramsType: "EngineRestartedEvent", durable: true),
    .ptydReconnected: EventDescriptor(paramsType: "PtydReconnectedEvent", durable: true),
    .ptydLost: EventDescriptor(paramsType: "PtydLostEvent", durable: true),
    .paneUpdated: EventDescriptor(paramsType: "PaneUpdatedEvent", durable: true),
    .settingsChanged: EventDescriptor(paramsType: "SettingsChangedEvent", durable: true),
    .paneSessionStarted: EventDescriptor(paramsType: "PaneSessionStartedEvent", durable: true),
    .paneTurnEnded: EventDescriptor(paramsType: "PaneTurnEndedEvent", durable: true),
    .paneQueued: EventDescriptor(paramsType: "PaneQueuedEvent", durable: true),
    .panePermissionRequested: EventDescriptor(paramsType: "PanePermissionRequestedEvent", durable: true),
    .panePermissionAnswered: EventDescriptor(paramsType: "PanePermissionAnsweredEvent", durable: true),
    .paneElicitationRequested: EventDescriptor(paramsType: "PaneElicitationRequestedEvent", durable: true),
    .paneElicitationAnswered: EventDescriptor(paramsType: "PaneElicitationAnsweredEvent", durable: true),
    .paneConfigChanging: EventDescriptor(paramsType: "PaneConfigChangingEvent", durable: true),
    .paneConfigChanged: EventDescriptor(paramsType: "PaneConfigChangedEvent", durable: true),
    .paneHarnessSwitchStarted: EventDescriptor(paramsType: "PaneHarnessSwitchStartedEvent", durable: true),
    .paneHarnessSwitchDone: EventDescriptor(paramsType: "PaneHarnessSwitchDoneEvent", durable: true),
    .paneHarnessSwitchFailed: EventDescriptor(paramsType: "PaneHarnessSwitchFailedEvent", durable: true),
    .paneConfigFailed: EventDescriptor(paramsType: "PaneConfigFailedEvent", durable: true),
    .paneSessionLost: EventDescriptor(paramsType: "PaneSessionLostEvent", durable: true),
    .paneNeedsLogin: EventDescriptor(paramsType: "PaneNeedsLoginEvent", durable: true),
    .paneAuthFinished: EventDescriptor(paramsType: "PaneAuthFinishedEvent", durable: true),
    .paneUpdate: EventDescriptor(paramsType: "PaneUpdateEvent", durable: true),
    .attentionChanged: EventDescriptor(paramsType: "AttentionChangedEvent", durable: true),
    .paneQueuePaused: EventDescriptor(paramsType: "PaneQueueStateEvent", durable: true),
    .paneQueueResumed: EventDescriptor(paramsType: "PaneQueueStateEvent", durable: true),
    .taskCreated: EventDescriptor(paramsType: "TaskCreatedEvent", durable: true),
    .taskMoved: EventDescriptor(paramsType: "TaskMovedEvent", durable: true),
    .taskClaimed: EventDescriptor(paramsType: "TaskClaimedEvent", durable: true),
    .taskReleased: EventDescriptor(paramsType: "TaskReleasedEvent", durable: true),
    .taskCommented: EventDescriptor(paramsType: "TaskCommentedEvent", durable: true),
    .taskSubitemChanged: EventDescriptor(paramsType: "TaskSubitemChangedEvent", durable: true),
    .taskBlocked: EventDescriptor(paramsType: "TaskBlockedEvent", durable: true),
    .taskUnblocked: EventDescriptor(paramsType: "TaskUnblockedEvent", durable: true),
    .taskPhraseChanged: EventDescriptor(paramsType: "TaskPhraseChangedEvent", durable: true),
    .taskUpdated: EventDescriptor(paramsType: "TaskUpdatedEvent", durable: true),
    .taskAttention: EventDescriptor(paramsType: "TaskAttentionEvent", durable: true),
    .loopTick: EventDescriptor(paramsType: "LoopTickEvent", durable: true),
    .loopStopped: EventDescriptor(paramsType: "LoopStoppedEvent", durable: true),
    .usageChanged: EventDescriptor(paramsType: "UsageChangedEvent", durable: true),
    .fileChangedOnDisk: EventDescriptor(paramsType: "FileChangedOnDiskEvent", durable: true),
    .fileSaved: EventDescriptor(paramsType: "FileSavedEvent", durable: true),
    .fileConflict: EventDescriptor(paramsType: "FileConflictEvent", durable: true),
    .lspDiagnostics: EventDescriptor(paramsType: "LspDiagnosticsEvent", durable: true),
    .editorAgentCursor: EventDescriptor(paramsType: "EditorAgentCursorEvent", durable: true),
    .editorReviewRequested: EventDescriptor(paramsType: "EditorReviewRequestedEvent", durable: true),
    .serversLocalChanged: EventDescriptor(paramsType: "ServersLocalChangedEvent", durable: false),
    .webNavigated: EventDescriptor(paramsType: "WebNavigatedEvent", durable: true),
    .webConsoleCounts: EventDescriptor(paramsType: "WebConsoleCountsEvent", durable: false),
    .webAgentDriving: EventDescriptor(paramsType: "WebAgentDrivingEvent", durable: false),
    .writingChanged: EventDescriptor(paramsType: "WritingChangedEvent", durable: true),
]
