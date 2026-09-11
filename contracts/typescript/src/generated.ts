// Generated from contracts/protocol.json (sha256 b2528ce7f18d51677445f8aafc989b17a0a00c8002ae0fe860c1807b31fae56d). Do not edit.
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type Id = string;

export type Decimal = string;

export type ByteString = string;

export type Role = "human_local" | "human_device" | "agent" | "chief";

export type PaneMode = "terminal" | "agent";

export type PaneState = "iniciando" | "trabalhando" | "ociosa" | "esperando_você" | "cancelando" | "sem_sinal" | "recuperando" | "morta" | "não_sei";

export type StateSource = "protocol" | "hook" | "osc" | "screen" | "none";

export type SnapshotSection = "workspaces" | "panes" | "tasks" | "requests" | "attention" | "devices" | "capabilities" | "streams";

export type ErrorKind = "not_found" | "invalid_params" | "unauthorized" | "forbidden_role" | "pane_busy" | "task_has_owner" | "transition_refused" | "stale_version" | "idempotency_conflict" | "harness_unavailable" | "needs_login" | "cursor_too_old" | "resync_required" | "slow_consumer" | "protocol_version_unsupported" | "disk_full" | "ptyd_unavailable" | "stale_engine_generation" | "internal_error" | "snapshot_too_large";

export interface RpcError {
  "code": number;
  "message": string;
  "data": ErrorData;
}

export interface ErrorData {
  "kind": ErrorKind;
  "detail"?: string;
}

export interface Actor {
  "role": Role;
  "client": string;
  "deviceId"?: Id;
  "paneId"?: Id;
}

export interface Workspace {
  "workspaceId": Id;
  "name": string;
  "path": string;
  "layout": JsonValue;
}

export interface Pane {
  "paneId": Id;
  "workspaceId": Id;
  "name": string;
  "cwd": string;
  "mode": PaneMode;
  "state": PaneState;
  "source": StateSource;
  "confidence": number;
  "phrase": string;
  "generation": number;
  "cols": number;
  "rows": number;
  "record": boolean;
  "pid"?: number;
  "streamId"?: Id;
}

export interface Device {
  "deviceId": Id;
  "name": string;
  "fingerprint": string;
  "revoked": boolean;
}

export interface StreamInfo {
  "streamId": Id;
  "paneId": Id;
  "generation": number;
  "firstRetainedOffset": Decimal;
  "nextOffset": Decimal;
  "binaryPaneRef": number;
}

export interface PtydStatus {
  "connected": boolean;
  "version": number;
  "generation": number;
}

export interface DbStatus {
  "id": Id;
  "schemaVersion": number;
}

export interface StreamCursor {
  "streamId": Id;
  "offset": Decimal;
}

export type ReplayFrom = TailSelector | StreamCursor;

export interface Welcome {
  "engineVersion": string;
  "protocolVersion": number;
  "connectionId": Id;
  "role": Role;
  "capabilities": Array<string>;
  "dbId": Id;
}

export interface Empty {

}

export interface HelloParams {
  "client": string;
  "clientVersion": string;
  "protocolVersion": number;
  "deviceToken"?: string;
  "proof"?: ByteString;
  "binding"?: string;
}

export interface StateSnapshotParams {

}

export interface StateSnapshotResult {
  "snapshotId": Id;
  "cursor": Decimal;
  "parts": number;
  "focus"?: ClientFocus;
}

export interface EventsSubscribeParams {
  "since": Decimal;
}

export interface EventsSubscribeResult {
  "subscriptionId": Id;
  "through": Decimal;
}

export interface WorkspaceListParams {

}

export interface WorkspaceListResult {
  "items": Array<Workspace>;
}

export interface WorkspaceCreateParams {
  "name": string;
  "path": string;
  "idempotencyKey": Id;
}

export interface WorkspaceCreateResult {
  "workspace": Workspace;
}

export interface WorkspaceRenameParams {
  "workspaceId": Id;
  "name": string;
  "idempotencyKey": Id;
}

export interface WorkspaceRenameResult {
  "workspace": Workspace;
}

export interface WorkspaceCloseParams {
  "workspaceId": Id;
  "idempotencyKey": Id;
}

export interface WorkspaceCloseResult {
  "workspaceId": Id;
}

export interface WorkspaceFocusParams {
  "workspaceId": Id;
  "idempotencyKey": Id;
}

export interface WorkspaceFocusResult {
  "workspaceId": Id;
}

export interface PaneCreateParams {
  "workspaceId": Id;
  "mode": PaneMode;
  "cwd"?: string;
  "name"?: string;
  "cols"?: number;
  "rows"?: number;
  "idempotencyKey": Id;
}

export interface PaneCreateResult {
  "paneId": Id;
}

export interface PaneCloseParams {
  "paneId": Id;
  "idempotencyKey": Id;
}

export interface PaneCloseResult {
  "paneId": Id;
}

export interface PaneFocusParams {
  "paneId": Id;
  "idempotencyKey": Id;
}

export interface PaneFocusResult {
  "paneId": Id;
}

export interface PaneRenameParams {
  "paneId": Id;
  "name": string;
  "idempotencyKey": Id;
}

export interface PaneRenameResult {
  "pane": Pane;
}

export interface PaneResizeParams {
  "paneId": Id;
  "cols": number;
  "rows": number;
  "idempotencyKey": Id;
}

export interface PaneResizeResult {
  "cols": number;
  "rows": number;
}

export interface PaneInputParams {
  "paneId": Id;
  "bytes": ByteString;
  "idempotencyKey": Id;
}

export interface PaneInputResult {
  "acceptedBytes": number;
}

export interface PaneStreamSubscribeParams {
  "paneId": Id;
  "from": ReplayFrom;
}

export interface PaneStreamUnsubscribeParams {
  "paneId": Id;
}

export interface PaneRecordSetParams {
  "paneId": Id;
  "on": boolean;
  "idempotencyKey": Id;
}

export interface PaneRecordSetResult {
  "on": boolean;
}

export interface DevicePairStartParams {
  "idempotencyKey": Id;
}

export interface DevicePairStartResult {
  "code": string;
  "expiresAt": string;
  "uri": string;
}

export interface DevicePairDecideParams {
  "requestId": Id;
  "approve": boolean;
  "idempotencyKey": Id;
}

export interface DevicePairDecideResult {
  "requestId": Id;
  "approved": boolean;
}

export interface DeviceListParams {

}

export interface DeviceListResult {
  "items": Array<Device>;
}

export interface DeviceRevokeParams {
  "deviceId": Id;
  "idempotencyKey": Id;
}

export interface DeviceRevokeResult {
  "deviceId": Id;
}

export interface PairBeginParams {
  "code": string;
  "deviceName": string;
  "devicePublicKey": ByteString;
  "idempotencyKey": Id;
}

export interface PairBeginResult {
  "requestId": Id;
  "challenge": ByteString;
}

export interface PairProveParams {
  "requestId": Id;
  "proof": ByteString;
  "idempotencyKey": Id;
}

export interface PairProveResult {
  "status": string;
}

export interface EngineStatusParams {

}

export interface EngineStatusResult {
  "version": string;
  "generation": number;
  "ptyd": PtydStatus;
  "db": DbStatus;
}

export interface SettingsGetParams {
  "key": string;
}

export interface SettingsGetResult {
  "key": string;
  "value": JsonValue;
}

export interface SettingsSetParams {
  "key": string;
  "value": JsonValue;
  "idempotencyKey": Id;
}

export interface SettingsSetResult {
  "key": string;
  "value": JsonValue;
}

export interface AuthChallengeEvent {
  "nonce": ByteString;
  "engineId": Id;
}

export interface StateSnapshotPartEvent {
  "snapshotId": Id;
  "index": number;
  "of": number;
  "section": SnapshotSection;
  "data": JsonValue;
}

export interface WorkspaceCreatedEvent {
  "eventId": Decimal;
  "at": string;
  "workspace": Workspace;
}

export interface WorkspaceRenamedEvent {
  "eventId": Decimal;
  "at": string;
  "workspace": Workspace;
}

export interface WorkspaceClosedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
}

export interface PaneCreatedEvent {
  "eventId": Decimal;
  "at": string;
  "pane": Pane;
}

export interface PaneRenamedEvent {
  "eventId": Decimal;
  "at": string;
  "pane": Pane;
}

export interface PaneClosedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "workspaceId": Id;
}

export interface PaneStateChangedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "state": PaneState;
  "confidence": number;
  "phrase": string;
  "source": StateSource;
}

export interface StreamReplayStartEvent {
  "streamId": Id;
  "generation": number;
  "fromOffset": Decimal;
  "toOffset": Decimal;
  "reset": boolean;
}

export interface StreamReplayDoneEvent {
  "streamId": Id;
  "nextOffset": Decimal;
}

export interface StreamGapEvent {
  "streamId": Id;
  "fromOffset": Decimal;
  "toOffset": Decimal;
}

export interface DevicePairRequestedEvent {
  "eventId": Decimal;
  "at": string;
  "requestId": Id;
  "deviceName": string;
  "fingerprint": string;
}

export interface DevicePairedEvent {
  "eventId": Decimal;
  "at": string;
  "device": Device;
}

export interface DeviceRevokedEvent {
  "eventId": Decimal;
  "at": string;
  "deviceId": Id;
}

export interface PairResultEvent {
  "requestId": Id;
  "approved": boolean;
  "deviceToken"?: string;
  "deviceId"?: Id;
  "reason"?: string;
}

export interface EngineUpdatingEvent {
  "eventId": Decimal;
  "at": string;
  "generation": number;
}

export interface EngineRestartedEvent {
  "eventId": Decimal;
  "at": string;
  "generation": number;
}

export interface PtydReconnectedEvent {
  "eventId": Decimal;
  "at": string;
  "generation": number;
}

export interface PtydLostEvent {
  "eventId": Decimal;
  "at": string;
  "reason": string;
}

export type TailSelector = "tail";

export type RpcVersion = "2.0";

export type RpcStringId = string;

export type RpcNumericId = number;

export type RpcNullId = null;

export type RpcId = RpcStringId | RpcNumericId | RpcNullId;

export interface RequestEnvelope {
  "jsonrpc": RpcVersion;
  "id": RpcId;
  "method": string;
  "params": JsonValue;
}

export interface NotificationEnvelope {
  "jsonrpc": RpcVersion;
  "method": string;
  "params": JsonValue;
}

export interface SuccessEnvelope {
  "jsonrpc": RpcVersion;
  "id": RpcId;
  "result": JsonValue;
}

export interface ErrorEnvelope {
  "jsonrpc": RpcVersion;
  "id": RpcId;
  "error": RpcError;
}

export interface PaneUpdatedEvent {
  "eventId": Decimal;
  "at": string;
  "pane": Pane;
}

export interface SettingsChangedEvent {
  "eventId": Decimal;
  "at": string;
  "key": string;
  "value": JsonValue;
}

export interface DevicePairPending {
  "kind": string;
  "requestId": Id;
  "deviceName": string;
  "fingerprint": string;
  "expiresAt": string;
}

export interface ClientFocus {
  "workspaceId"?: Id;
  "paneId"?: Id;
}

export interface Methods {
  "hello": { params: HelloParams; result: Welcome };
  "state.snapshot": { params: StateSnapshotParams; result: StateSnapshotResult };
  "events.subscribe": { params: EventsSubscribeParams; result: EventsSubscribeResult };
  "workspace.list": { params: WorkspaceListParams; result: WorkspaceListResult };
  "workspace.create": { params: WorkspaceCreateParams; result: WorkspaceCreateResult };
  "workspace.rename": { params: WorkspaceRenameParams; result: WorkspaceRenameResult };
  "workspace.close": { params: WorkspaceCloseParams; result: WorkspaceCloseResult };
  "workspace.focus": { params: WorkspaceFocusParams; result: WorkspaceFocusResult };
  "pane.create": { params: PaneCreateParams; result: PaneCreateResult };
  "pane.close": { params: PaneCloseParams; result: PaneCloseResult };
  "pane.focus": { params: PaneFocusParams; result: PaneFocusResult };
  "pane.rename": { params: PaneRenameParams; result: PaneRenameResult };
  "pane.resize": { params: PaneResizeParams; result: PaneResizeResult };
  "pane.input": { params: PaneInputParams; result: PaneInputResult };
  "pane.stream.subscribe": { params: PaneStreamSubscribeParams; result: StreamInfo };
  "pane.stream.unsubscribe": { params: PaneStreamUnsubscribeParams; result: Empty };
  "pane.record.set": { params: PaneRecordSetParams; result: PaneRecordSetResult };
  "device.pair.start": { params: DevicePairStartParams; result: DevicePairStartResult };
  "device.pair.decide": { params: DevicePairDecideParams; result: DevicePairDecideResult };
  "device.list": { params: DeviceListParams; result: DeviceListResult };
  "device.revoke": { params: DeviceRevokeParams; result: DeviceRevokeResult };
  "pair.begin": { params: PairBeginParams; result: PairBeginResult };
  "pair.prove": { params: PairProveParams; result: PairProveResult };
  "engine.status": { params: EngineStatusParams; result: EngineStatusResult };
  "settings.get": { params: SettingsGetParams; result: SettingsGetResult };
  "settings.set": { params: SettingsSetParams; result: SettingsSetResult };
}
