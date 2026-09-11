// Generated from contracts/protocol.json (sha256 e7fc8a470dbcdd7f1dba14e02ec4b645f704e8520280ab04c19f187e38f9a984). Do not edit.
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

public enum PaneMode: String, Codable, Sendable {
    case `terminal` = "terminal"
    case `agent` = "agent"
}

public enum PaneState: String, Codable, Sendable {
    case `iniciando` = "iniciando"
    case `trabalhando` = "trabalhando"
    case `ociosa` = "ociosa"
    case `esperando_você` = "esperando_você"
    case `cancelando` = "cancelando"
    case `sem_sinal` = "sem_sinal"
    case `recuperando` = "recuperando"
    case `morta` = "morta"
    case `não_sei` = "não_sei"
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
    case `devices` = "devices"
    case `capabilities` = "capabilities"
    case `streams` = "streams"
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
    public init(`kind`: ErrorKind, `detail`: String? = nil) {
        self.`kind` = `kind`
        self.`detail` = `detail`
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
    public var `pid`: UInt32?
    public var `streamId`: Id?
    public var `agent`: AgentSession?
    public init(`paneId`: Id, `workspaceId`: Id, `name`: String, `cwd`: String, `mode`: PaneMode, `state`: PaneState, `source`: StateSource, `confidence`: Double, `phrase`: String, `generation`: UInt32, `cols`: UInt32, `rows`: UInt32, `record`: Bool, `pid`: UInt32? = nil, `streamId`: Id? = nil, `agent`: AgentSession? = nil) {
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
        self.`pid` = `pid`
        self.`streamId` = `streamId`
        self.`agent` = `agent`
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
    public init(`client`: String, `clientVersion`: String, `protocolVersion`: UInt32, `deviceToken`: String? = nil, `proof`: ByteString? = nil, `binding`: String? = nil) {
        self.`client` = `client`
        self.`clientVersion` = `clientVersion`
        self.`protocolVersion` = `protocolVersion`
        self.`deviceToken` = `deviceToken`
        self.`proof` = `proof`
        self.`binding` = `binding`
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
    public init(`workspaceId`: Id, `mode`: PaneMode, `cwd`: String? = nil, `name`: String? = nil, `cols`: UInt32? = nil, `rows`: UInt32? = nil, `idempotencyKey`: Id, `harness`: Id? = nil, `model`: String? = nil, `effort`: String? = nil, `permission`: PermissionMode? = nil) {
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
    case `funcionou` = "funcionou"
    case `não_suportado` = "não_suportado"
    case `não_autenticado` = "não_autenticado"
    case `inconclusivo` = "inconclusivo"
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
    public init(`promptId`: Id, `text`: String, `origin`: PromptOrigin, `status`: PromptStatus, `queuedAt`: String, `generation`: UInt32? = nil) {
        self.`promptId` = `promptId`
        self.`text` = `text`
        self.`origin` = `origin`
        self.`status` = `status`
        self.`queuedAt` = `queuedAt`
        self.`generation` = `generation`
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
    public init(`requestId`: Id, `paneId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `detail`: String, `options`: [PermissionOption], `expiresAt`: String) {
        self.`requestId` = `requestId`
        self.`paneId` = `paneId`
        self.`generation` = `generation`
        self.`promptId` = `promptId`
        self.`title` = `title`
        self.`detail` = `detail`
        self.`options` = `options`
        self.`expiresAt` = `expiresAt`
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

public struct AgentSession: Codable, Sendable {
    public var `harness`: Id
    public var `version`: String
    public var `fingerprint`: String
    public var `sessionId`: Id?
    public var `options`: [ConfigOption]
    public var `permission`: PermissionMode
    public var `yolo`: Bool
    public var `previousPermission`: PermissionMode?
    public var `effectiveMode`: String?
    public var `operation`: ConfigOperation?
    public var `queue`: [PromptItem]
    public var `draft`: Bool
    public var `activePromptId`: Id?
    public init(`harness`: Id, `version`: String, `fingerprint`: String, `sessionId`: Id? = nil, `options`: [ConfigOption], `permission`: PermissionMode, `yolo`: Bool, `previousPermission`: PermissionMode? = nil, `effectiveMode`: String? = nil, `operation`: ConfigOperation? = nil, `queue`: [PromptItem], `draft`: Bool, `activePromptId`: Id? = nil) {
        self.`harness` = `harness`
        self.`version` = `version`
        self.`fingerprint` = `fingerprint`
        self.`sessionId` = `sessionId`
        self.`options` = `options`
        self.`permission` = `permission`
        self.`yolo` = `yolo`
        self.`previousPermission` = `previousPermission`
        self.`effectiveMode` = `effectiveMode`
        self.`operation` = `operation`
        self.`queue` = `queue`
        self.`draft` = `draft`
        self.`activePromptId` = `activePromptId`
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
    public var `version`: String?
    public var `fingerprint`: String?
    public var `capabilities`: [ProbeCapability]
    public var `options`: [ConfigOption]
    public var `authMethods`: [AuthMethod]
    public init(`id`: Id, `name`: String, `available`: Bool, `version`: String? = nil, `fingerprint`: String? = nil, `capabilities`: [ProbeCapability], `options`: [ConfigOption], `authMethods`: [AuthMethod]) {
        self.`id` = `id`
        self.`name` = `name`
        self.`available` = `available`
        self.`version` = `version`
        self.`fingerprint` = `fingerprint`
        self.`capabilities` = `capabilities`
        self.`options` = `options`
        self.`authMethods` = `authMethods`
    }
}

public struct ModelInfo: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `description`: String?
    public init(`id`: Id, `name`: String, `description`: String? = nil) {
        self.`id` = `id`
        self.`name` = `name`
        self.`description` = `description`
    }
}

public struct HarnessLaunch: Codable, Sendable {
    public var `id`: Id
    public var `name`: String
    public var `command`: String
    public var `args`: [String]
    public var `version`: String
    public init(`id`: Id, `name`: String, `command`: String, `args`: [String], `version`: String) {
        self.`id` = `id`
        self.`name` = `name`
        self.`command` = `command`
        self.`args` = `args`
        self.`version` = `version`
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
    public init(`paneId`: Id) {
        self.`paneId` = `paneId`
    }
}

public struct PaneQueueListResult: Codable, Sendable {
    public var `items`: [PromptItem]
    public var `draft`: Bool
    public init(`items`: [PromptItem], `draft`: Bool) {
        self.`items` = `items`
        self.`draft` = `draft`
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

    public init() {

    }
}

public struct HarnessListResult: Codable, Sendable {
    public var `items`: [HarnessInfo]
    public init(`items`: [HarnessInfo]) {
        self.`items` = `items`
    }
}

public struct ModelListParams: Codable, Sendable {
    public var `harness`: Id
    public var `paneId`: Id?
    public init(`harness`: Id, `paneId`: Id? = nil) {
        self.`harness` = `harness`
        self.`paneId` = `paneId`
    }
}

public struct ModelListResult: Codable, Sendable {
    public var `items`: [ModelInfo]
    public var `options`: [ConfigOption]
    public var `source`: String
    public init(`items`: [ModelInfo], `options`: [ConfigOption], `source`: String) {
        self.`items` = `items`
        self.`options` = `options`
        self.`source` = `source`
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
    public init(`eventId`: Decimal, `at`: String, `paneId`: Id, `requestId`: Id, `generation`: UInt32, `promptId`: Id, `title`: String, `detail`: String, `options`: [PermissionOption], `expiresAt`: String) {
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
