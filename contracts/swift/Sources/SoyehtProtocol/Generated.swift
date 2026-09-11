// Generated from contracts/protocol.json (sha256 3f6aaefb6d35b20b749b5da4bbd0c3fe8b0a28145d875f5bb05e90f470198ae2). Do not edit.
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
    public init(`paneId`: Id, `workspaceId`: Id, `name`: String, `cwd`: String, `mode`: PaneMode, `state`: PaneState, `source`: StateSource, `confidence`: Double, `phrase`: String, `generation`: UInt32, `cols`: UInt32, `rows`: UInt32, `record`: Bool, `pid`: UInt32? = nil, `streamId`: Id? = nil) {
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
    public init(`snapshotId`: Id, `cursor`: Decimal, `parts`: UInt32) {
        self.`snapshotId` = `snapshotId`
        self.`cursor` = `cursor`
        self.`parts` = `parts`
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
    public init(`workspaceId`: Id, `mode`: PaneMode, `cwd`: String? = nil, `name`: String? = nil, `cols`: UInt32? = nil, `rows`: UInt32? = nil, `idempotencyKey`: Id) {
        self.`workspaceId` = `workspaceId`
        self.`mode` = `mode`
        self.`cwd` = `cwd`
        self.`name` = `name`
        self.`cols` = `cols`
        self.`rows` = `rows`
        self.`idempotencyKey` = `idempotencyKey`
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
