// Generated from contracts/protocol.json (sha256 6e36ed94149695bac6694bf3d4201eb951cc7d0fedd3fa6e47e970f9799bbc64). Do not edit.
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type Id = string;

export type Decimal = string;

export type ByteString = string;

export type Role = "human_local" | "human_device" | "agent" | "chief";

export type AgentPaneRole = "worker" | "chief";

export type PaneMode = "terminal" | "agent" | "editor" | "web";

export type PaneState = "starting" | "working" | "idle" | "waiting_for_you" | "cancelling" | "no_signal" | "recovering" | "dead" | "unknown";

export type StateSource = "protocol" | "hook" | "osc" | "screen" | "none";

export type SnapshotSection = "workspaces" | "panes" | "tasks" | "requests" | "attention" | "loops" | "devices" | "capabilities" | "streams" | "files";

export type ErrorKind = "not_found" | "invalid_params" | "unauthorized" | "forbidden_role" | "pane_busy" | "task_has_owner" | "transition_refused" | "stale_version" | "idempotency_conflict" | "harness_unavailable" | "needs_login" | "cursor_too_old" | "resync_required" | "slow_consumer" | "protocol_version_unsupported" | "disk_full" | "ptyd_unavailable" | "stale_engine_generation" | "internal_error" | "snapshot_too_large" | "engine_identity_changed" | "stale_attempt" | "dependency_cycle" | "dependency_open" | "rate_limited" | "stale_reference" | "human_took_over" | "web_host_unavailable" | "navigation_refused";

export interface RpcError {
  "code": number;
  "message": string;
  "data": ErrorData;
}

export interface ErrorData {
  "kind": ErrorKind;
  "detail"?: string;
  "retryAfterSec"?: number;
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
  "agent"?: AgentSession;
  "url"?: string;
  "title"?: string;
  "viewport"?: WebViewport;
  "incognito"?: boolean;
  "webSession"?: WebSessionState;
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
  "capabilities"?: Array<ClientCapability>;
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
  "harness"?: Id;
  "model"?: string;
  "effort"?: string;
  "permission"?: PermissionMode;
  "url"?: string;
  "incognito"?: boolean;
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

export type PermissionMode = "ask" | "accept_edits" | "allow_all";

export type PromptOrigin = "human" | "automatic";

export type PromptStatus = "queued" | "active" | "completed" | "cancelled";

export type TranscriptRole = "user" | "assistant" | "system";

export type TranscriptKind = "message" | "thought" | "tool" | "terminal" | "plan" | "usage" | "system";

export type PermissionOptionKind = "allow_once" | "allow_always" | "reject_once" | "reject_always";

export type PermissionResult = "selected" | "cancelled" | "expired";

export type ElicitationResult = "submitted" | "cancelled" | "expired";

export type DecisionBy = "mac" | "iphone" | "rule" | "system";

export type ProbeResult = "worked" | "unsupported" | "unauthenticated" | "inconclusive";

export type ConfigType = "select" | "boolean";

export type ToolStatus = "pending" | "in_progress" | "completed" | "failed";

export type PlanStatus = "pending" | "in_progress" | "completed";

export type PlanPriority = "high" | "medium" | "low";

export type SessionRecovery = "resumed" | "new_session" | "needs_login";

export type ConfigOperationKind = "config" | "yolo";

export type ConfigValue = string | boolean;

export interface ConfigChoice {
  "value": Id;
  "name": string;
  "description"?: string;
}

export interface ConfigOption {
  "id": Id;
  "name": string;
  "description"?: string;
  "category"?: string;
  "type": ConfigType;
  "currentValue": ConfigValue;
  "options": Array<ConfigChoice>;
}

export interface ConfigOperation {
  "operationId": Id;
  "kind": ConfigOperationKind;
  "configId": Id;
  "value": ConfigValue;
  "startedAt": string;
}

export interface PromptItem {
  "promptId": Id;
  "text": string;
  "origin": PromptOrigin;
  "status": PromptStatus;
  "queuedAt": string;
  "generation"?: number;
  "attachments"?: Array<AttachmentRef>;
}

export interface PermissionOption {
  "optionId": Id;
  "kind": PermissionOptionKind;
  "name": string;
}

export interface PermissionRequest {
  "requestId": Id;
  "paneId": Id;
  "generation": number;
  "promptId": Id;
  "title": string;
  "detail": string;
  "options": Array<PermissionOption>;
  "expiresAt": string;
  "kind"?: PermissionRequestKind;
  "domain"?: string;
}

export interface ElicitationRequest {
  "requestId": Id;
  "paneId": Id;
  "generation": number;
  "promptId": Id;
  "title": string;
  "schema": JsonValue;
  "expiresAt": string;
}

export interface AgentSession {
  "harness": Id;
  "version": string;
  "fingerprint": string;
  "sessionId"?: Id;
  "options": Array<ConfigOption>;
  "authMethods": Array<AuthMethod>;
  "permission": PermissionMode;
  "yolo": boolean;
  "previousPermission"?: PermissionMode;
  "effectiveMode"?: string;
  "operation"?: ConfigOperation;
  "queue": Array<QueuedPromptSummary>;
  "draft": boolean;
  "activePromptId"?: Id;
  "queuePaused"?: boolean;
  "role": AgentPaneRole;
}

export interface ProbeCapability {
  "method": string;
  "result": ProbeResult;
  "checkedAt": string;
}

export interface AuthMethod {
  "id": Id;
  "name": string;
  "description"?: string;
  "terminal": boolean;
}

export interface HarnessInfo {
  "id": Id;
  "name": string;
  "available": boolean;
  "version"?: string;
  "fingerprint"?: string;
  "capabilities": Array<ProbeCapability>;
  "options": Array<ConfigOption>;
  "authMethods": Array<AuthMethod>;
}

export interface ModelInfo {
  "id": Id;
  "name": string;
  "description"?: string;
}

export type ModelListSource = "not_probed" | "native_session";

export interface HarnessLaunch {
  "id": Id;
  "name": string;
  "command": string;
  "args": Array<string>;
  "version": string;
}

export type HarnessConfiguration = Array<HarnessLaunch>;

export interface ToolLocation {
  "path": string;
  "line"?: number;
}

export interface ToolCall {
  "toolCallId": Id;
  "title": string;
  "kind"?: string;
  "status": ToolStatus;
  "content"?: JsonValue;
  "locations": Array<ToolLocation>;
  "rawInput"?: JsonValue;
  "rawOutput"?: JsonValue;
}

export interface PlanEntry {
  "content": string;
  "priority": PlanPriority;
  "status": PlanStatus;
}

export interface TokenUsage {
  "inputTokens"?: Decimal;
  "outputTokens"?: Decimal;
  "totalTokens": Decimal;
  "contextSize"?: Decimal;
}

export interface TerminalOutput {
  "terminalId": Id;
  "bytes": ByteString;
  "offset": Decimal;
  "truncated": boolean;
  "exitCode"?: number;
}

export interface TranscriptMessage {
  "localId": Id;
  "generation": number;
  "seq": Decimal;
  "role": TranscriptRole;
  "kind": TranscriptKind;
  "text": string;
  "complete": boolean;
  "promptId"?: Id;
  "tool"?: ToolCall;
  "terminal"?: TerminalOutput;
  "plan"?: Array<PlanEntry>;
  "usage"?: TokenUsage;
}

export interface AttachmentRef {
  "attachmentId": Id;
  "mimeType": string;
  "name": string;
  "bytes": Decimal;
}

export interface PanePromptParams {
  "paneId": Id;
  "text": string;
  "origin"?: PromptOrigin;
  "attachments"?: Array<AttachmentRef>;
  "idempotencyKey": Id;
}

export interface PanePromptResult {
  "promptId": Id;
  "queued": boolean;
}

export interface PaneQueueListParams {
  "paneId": Id;
  "cursor"?: Id;
  "limit"?: number;
}

export interface PaneQueueListResult {
  "items": Array<PromptItem>;
  "draft": boolean;
  "nextCursor"?: Id;
  "paused"?: boolean;
}

export interface PaneQueueRemoveParams {
  "paneId": Id;
  "promptId": Id;
  "idempotencyKey": Id;
}

export interface PaneQueueRemoveResult {
  "promptId": Id;
  "removed": boolean;
}

export interface PaneCancelParams {
  "paneId": Id;
  "idempotencyKey": Id;
}

export interface PaneCancelResult {
  "paneId": Id;
  "promptId"?: Id;
  "cancelling": boolean;
}

export interface PanePermissionAnswerParams {
  "paneId": Id;
  "requestId": Id;
  "optionId": Id;
  "idempotencyKey": Id;
}

export interface PanePermissionAnswerResult {
  "requestId": Id;
  "result": PermissionResult;
  "optionId"?: Id;
  "by": DecisionBy;
}

export interface PaneElicitationAnswerParams {
  "paneId": Id;
  "requestId": Id;
  "fields"?: JsonValue;
  "cancel"?: boolean;
  "idempotencyKey": Id;
}

export interface PaneElicitationAnswerResult {
  "requestId": Id;
  "result": ElicitationResult;
  "by": DecisionBy;
}

export interface PaneConfigSetParams {
  "paneId": Id;
  "configId": Id;
  "value": ConfigValue;
  "idempotencyKey": Id;
}

export interface PaneConfigSetResult {
  "applied": boolean;
  "effective": ConfigValue;
  "sessionPreserved": boolean;
}

export interface PaneYoloParams {
  "paneId": Id;
  "on": boolean;
  "idempotencyKey": Id;
}

export interface PaneYoloResult {
  "effectiveMode": PermissionMode;
  "on": boolean;
}

export interface PaneAuthStartParams {
  "paneId": Id;
  "authMethodId"?: Id;
  "idempotencyKey": Id;
}

export interface PaneAuthStartResult {
  "paneId": Id;
  "terminalPaneId"?: Id;
}

export interface PaneTranscriptParams {
  "paneId": Id;
  "before"?: Decimal;
  "limit"?: number;
}

export interface PaneTranscriptResult {
  "items": Array<TranscriptMessage>;
  "nextCursor"?: Decimal;
}

export interface PaneDraftParams {
  "paneId": Id;
  "active": boolean;
  "idempotencyKey": Id;
}

export interface PaneDraftResult {
  "active": boolean;
}

export interface HarnessListParams {
  "cursor"?: Id;
  "limit"?: number;
}

export interface HarnessListResult {
  "items": Array<HarnessInfo>;
  "nextCursor"?: Id;
}

export interface ModelListParams {
  "harness": Id;
  "paneId"?: Id;
  "cursor"?: Id;
  "limit"?: number;
}

export interface ModelListResult {
  "items": Array<ModelInfo>;
  "options": Array<ConfigOption>;
  "source": ModelListSource;
  "nextCursor"?: Id;
}

export interface PaneSessionStartedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "harness": Id;
  "version": string;
  "sessionId": Id;
  "generation": number;
  "options": Array<ConfigOption>;
}

export interface PaneTurnEndedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "promptId": Id;
  "stopReason": TurnStopReason;
}

export interface PaneQueuedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "n": number;
  "promptIds": Array<Id>;
  "draft": boolean;
}

export interface PanePermissionRequestedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "requestId": Id;
  "generation": number;
  "promptId": Id;
  "title": string;
  "detail": string;
  "options": Array<PermissionOption>;
  "expiresAt": string;
  "kind"?: PermissionRequestKind;
  "domain"?: string;
}

export interface PanePermissionAnsweredEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "requestId": Id;
  "result": PermissionResult;
  "optionId"?: Id;
  "by": DecisionBy;
}

export interface PaneElicitationRequestedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "requestId": Id;
  "generation": number;
  "promptId": Id;
  "title": string;
  "schema": JsonValue;
  "expiresAt": string;
}

export interface PaneElicitationAnsweredEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "requestId": Id;
  "result": ElicitationResult;
  "by": DecisionBy;
}

export interface PaneConfigChangingEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "configId": Id;
  "value": ConfigValue;
  "operationId": Id;
}

export interface PaneConfigChangedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "configId": Id;
  "value": ConfigValue;
  "options": Array<ConfigOption>;
  "sessionPreserved": boolean;
}

export interface PaneConfigFailedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "configId": Id;
  "value": ConfigValue;
  "reason": string;
}

export interface PaneSessionLostEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "reason": string;
  "recovery": SessionRecovery;
}

export interface PaneNeedsLoginEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "harness": Id;
  "authMethods": Array<AuthMethod>;
}

export interface PaneAuthFinishedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "ok": boolean;
  "reason"?: string;
}

export type AgentMessageUpdateKind = "agent_message_chunk";

export interface AgentMessageUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": AgentMessageUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "text": string;
  "complete": boolean;
}

export type AgentThoughtUpdateKind = "agent_thought_chunk";

export interface AgentThoughtUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": AgentThoughtUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "text": string;
  "complete": boolean;
}

export type ToolCallUpdateKind = "tool_call";

export interface ToolCallUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": ToolCallUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "tool": ToolCall;
}

export type ToolProgressUpdateKind = "tool_call_update";

export interface ToolProgressUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": ToolProgressUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "tool": ToolCall;
}

export type PlanUpdateKind = "plan";

export interface PlanUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": PlanUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "entries": Array<PlanEntry>;
}

export type UsageUpdateKind = "usage";

export interface UsageUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": UsageUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "usage": TokenUsage;
}

export type TerminalOutputUpdateKind = "terminal_output";

export interface TerminalOutputUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": TerminalOutputUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "terminalId": Id;
  "bytes": ByteString;
  "offset": Decimal;
  "truncated": boolean;
}

export type ConfigOptionUpdateKind = "config_option_update";

export interface ConfigOptionUpdate {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "kind": ConfigOptionUpdateKind;
  "generation": number;
  "seq": Decimal;
  "localId": Id;
  "options": Array<ConfigOption>;
}

export type PaneUpdateEvent = AgentMessageUpdate | AgentThoughtUpdate | ToolCallUpdate | ToolProgressUpdate | PlanUpdate | UsageUpdate | TerminalOutputUpdate | ConfigOptionUpdate;

export interface AttentionItem {
  "attentionId": Id;
  "paneId": Id;
  "requestId"?: Id;
  "message": string;
  "createdAt": string;
}

export interface AttentionChangedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "count": number;
  "items": Array<AttentionItem>;
}

export type TurnStopReason = "end_turn" | "max_tokens" | "max_turn_requests" | "refusal" | "cancelled" | "error";

export interface QueuedPromptSummary {
  "promptId": Id;
  "origin": PromptOrigin;
  "status": PromptStatus;
  "queuedAt": string;
  "generation"?: number;
}

export interface PaneQueueResumeParams {
  "paneId": Id;
  "idempotencyKey": Id;
}

export interface PaneQueueResumeResult {
  "paneId": Id;
  "paused": boolean;
}

export interface PaneQueueStateEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "paused": boolean;
  "reason"?: string;
}

export type TaskState = "todo" | "in_progress" | "in_review" | "completed" | "cancelled";

export type TaskPriority = "urgent" | "high" | "medium" | "low";

export type SubitemState = "open" | "done";

export type AttemptResult = "completed" | "returned" | "failed" | "cancelled";

export type BlockingKind = "dependency" | "missing_information" | "external_error";

export type TaskCheckState = "unknown" | "pending" | "green" | "red" | "conflict";

export type TaskReviewState = "none" | "awaiting_person" | "changes_requested" | "approved";

export type TaskReviewBadge = "system_turn" | "your_turn";

export type LoopStopWhen = "round_limit" | "three_empty" | "explicit_off";

export type DriftLabel = "equivalent" | "style" | "algorithm" | "input_gating" | "silent_fallback" | "error_handling" | "return_shape" | "external_effects" | "extra_edits" | "incomplete";

export interface TaskExecutor {
  "workspaceDefault": boolean;
  "harness"?: Id;
  "model"?: string;
  "effort"?: string;
  "permission"?: PermissionMode;
}

export interface TaskWorktree {
  "path": string;
  "branch": string;
}

export interface TaskPullRequest {
  "url": string;
  "checkState": TaskCheckState;
  "reviewState": TaskReviewState;
  "head"?: string;
}

export interface Blocking {
  "kind": BlockingKind;
  "reason": string;
  "dependencyTaskId"?: Id;
  "retryCount"?: number;
}

export interface Subitem {
  "subitemId": Id;
  "taskId": Id;
  "text": string;
  "agent"?: Id;
  "instructions"?: string;
  "state": SubitemState;
}

export interface Comment {
  "commentId": Id;
  "taskId": Id;
  "author": string;
  "text": string;
  "at": string;
}

export interface Attempt {
  "n": number;
  "paneId": Id;
  "harness": Id;
  "model": string;
  "start": string;
  "end"?: string;
  "result"?: AttemptResult;
  "summary"?: string;
}

export interface TaskMove {
  "actor": string;
  "from": TaskState;
  "to": TaskState;
  "reason": string;
  "at": string;
}

export interface ContractChange {
  "path": string;
  "name": string;
  "label": DriftLabel;
  "contractRef": string;
}

export interface OutOfScopeEdit {
  "path": string;
  "reason": string;
}

export interface LoopConfig {
  "prompt": string;
  "interval": number;
  "maxRounds": number;
  "stopWhen": LoopStopWhen;
  "enabled": boolean;
  "rounds"?: number;
  "emptyResults"?: number;
}

export interface Task {
  "workspaceId": Id;
  "taskId": Id;
  "title": string;
  "description": string;
  "howToVerify"?: string;
  "mustNotChange": string;
  "state": TaskState;
  "priority": TaskPriority;
  "plannedOwner"?: Id;
  "ownerPaneId"?: Id;
  "executor"?: TaskExecutor;
  "dependencies": Array<string>;
  "worktree"?: TaskWorktree;
  "currentAttempt": number;
  "blocking"?: Blocking;
  "pr"?: TaskPullRequest;
  "prContract": boolean;
  "createdAt": string;
  "createdBy": string;
  "version": number;
  "phrase": string;
  "reviewBadge"?: TaskReviewBadge;
  "summary"?: string;
  "contractChanges"?: Array<ContractChange>;
  "outOfScopeEdits"?: Array<OutOfScopeEdit>;
}

export interface TaskCreateParams {
  "workspaceId": Id;
  "title": string;
  "description"?: string;
  "howToVerify"?: string;
  "mustNotChange": string;
  "executor"?: TaskExecutor;
  "priority"?: TaskPriority;
  "createWorktree"?: boolean;
  "permission"?: PermissionMode;
  "dependencies"?: Array<string>;
  "dispatch"?: boolean;
  "idempotencyKey": Id;
}

export interface TaskCreateResult {
  "task": Task;
  "paneId"?: Id;
}

export interface TaskNextParams {
  "workspaceId": Id;
  "paneId": Id;
  "sessionGeneration": number;
  "idempotencyKey": Id;
}

export interface TaskNextResult {
  "task"?: Task;
  "empty": boolean;
  "willRestartSession": boolean;
}

export interface TaskShowParams {
  "workspaceId": Id;
  "taskId": Id;
}

export interface TaskShowResult {
  "task": Task;
  "subitems": Array<Subitem>;
  "comments": Array<Comment>;
  "attempts": Array<Attempt>;
  "history": Array<TaskMove>;
}

export interface TaskListParams {
  "workspaceId": Id;
  "state"?: TaskState;
  "limit": number;
  "cursor"?: Id;
}

export interface TaskListResult {
  "items": Array<Task>;
  "nextCursor"?: Id;
}

export interface TaskCommentsParams {
  "workspaceId": Id;
  "taskId": Id;
  "before"?: Id;
  "limit": number;
}

export interface TaskCommentsResult {
  "items": Array<Comment>;
  "nextCursor"?: Id;
}

export interface TaskUpdateParams {
  "workspaceId": Id;
  "taskId": Id;
  "version": number;
  "title"?: string;
  "description"?: string;
  "howToVerify"?: string;
  "mustNotChange"?: string;
  "priority"?: TaskPriority;
  "dependencies"?: Array<string>;
  "plannedOwner"?: Id;
  "idempotencyKey": Id;
}

export interface TaskUpdateResult {
  "task": Task;
}

export interface TaskCommentParams {
  "workspaceId": Id;
  "taskId": Id;
  "text": string;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskCommentResult {
  "comment": Comment;
}

export interface TaskSubitemAddParams {
  "workspaceId": Id;
  "taskId": Id;
  "text": string;
  "agent"?: Id;
  "instructions"?: string;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskSubitemAddResult {
  "subitem": Subitem;
}

export interface TaskSubitemDoneParams {
  "workspaceId": Id;
  "taskId": Id;
  "subitemId": Id;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskSubitemDoneResult {
  "subitem": Subitem;
}

export interface TaskBlockParams {
  "workspaceId": Id;
  "taskId": Id;
  "kind": BlockingKind;
  "reason": string;
  "dependencyTaskId"?: Id;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskBlockResult {
  "task": Task;
}

export interface TaskReleaseParams {
  "workspaceId": Id;
  "taskId": Id;
  "reason"?: string;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskReleaseResult {
  "task": Task;
}

export interface TaskRequestReviewParams {
  "workspaceId": Id;
  "taskId": Id;
  "reason"?: string;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskRequestReviewResult {
  "task": Task;
}

export interface TaskCompleteParams {
  "workspaceId": Id;
  "taskId": Id;
  "summary": string;
  "contractChanges": Array<ContractChange>;
  "outOfScopeEdits": Array<OutOfScopeEdit>;
  "paneId"?: Id;
  "sessionGeneration"?: number;
  "attempt"?: number;
  "idempotencyKey": Id;
}

export interface TaskCompleteResult {
  "task": Task;
}

export interface TaskDispatchParams {
  "workspaceId": Id;
  "taskId": Id;
  "preset": TaskExecutor;
  "idempotencyKey": Id;
}

export interface TaskDispatchResult {
  "task": Task;
  "paneId": Id;
  "attempt": number;
  "resumed": boolean;
}

export interface TaskUnblockParams {
  "workspaceId": Id;
  "taskId": Id;
  "reason"?: string;
  "idempotencyKey": Id;
}

export interface TaskUnblockResult {
  "task": Task;
}

export interface TaskMoveParams {
  "workspaceId": Id;
  "taskId": Id;
  "state": TaskState;
  "reason": string;
  "idempotencyKey": Id;
}

export interface TaskMoveResult {
  "task": Task;
}

export interface TaskWorktreeCleanParams {
  "workspaceId": Id;
  "taskId": Id;
  "idempotencyKey": Id;
}

export interface TaskWorktreeCleanResult {
  "cleaned": boolean;
  "reason"?: string;
}

export interface LoopSetParams {
  "paneId": Id;
  "prompt": string;
  "interval": number;
  "maxRounds": number;
  "stopWhen": LoopStopWhen;
  "enabled": boolean;
  "idempotencyKey": Id;
}

export interface LoopSetResult {
  "config": LoopConfig;
}

export interface TaskCreatedEvent {
  "eventId": Decimal;
  "at": string;
  "task": Task;
}

export interface TaskMovedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "actor": string;
  "from": TaskState;
  "to": TaskState;
  "reason": string;
}

export interface TaskClaimedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "paneId": Id;
  "attempt": number;
}

export interface TaskReleasedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "reason": string;
  "attempt": number;
}

export interface TaskCommentedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "comment": Comment;
}

export interface TaskSubitemChangedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "subitem": Subitem;
}

export interface TaskBlockedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "kind": BlockingKind;
  "reason": string;
}

export interface TaskUnblockedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "reason": string;
}

export interface TaskPhraseChangedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "phrase": string;
}

export interface TaskUpdatedEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "version": number;
}

export interface TaskAttentionEvent {
  "eventId": Decimal;
  "at": string;
  "workspaceId": Id;
  "taskId": Id;
  "n": number;
}

export interface LoopTickEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "round": number;
  "of": number;
}

export interface LoopStoppedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "reason": string;
}

export interface PaneRoleSetParams {
  "paneId": Id;
  "role": AgentPaneRole;
  "idempotencyKey": Id;
}

export interface PaneRoleSetResult {
  "paneId": Id;
  "role": AgentPaneRole;
}

export type UsageProvider = "claude" | "codex" | "cursor";

export type UsageState = "ok" | "stale" | "login_needed" | "no_access" | "unavailable";

export type UsageSource = "claude_cli_usage" | "claude_oauth_endpoint" | "claude_desktop_cache" | "codex_app_server" | "codex_account_endpoint" | "cursor_current_period";

export interface UsageWindow {
  "used": number;
  "resetsAt": string;
  "label": string;
}

export interface UsageReading {
  "provider": UsageProvider;
  "state": UsageState;
  "primary"?: UsageWindow;
  "secondary"?: UsageWindow;
  "source": UsageSource;
  "fetchedAt": string;
}

export interface UsageListParams {

}

export interface UsageListResult {
  "providers": Array<UsageReading>;
}

export interface UsageRefreshParams {
  "provider"?: UsageProvider;
  "idempotencyKey": Id;
}

export interface UsageRefreshResult {
  "providers": Array<UsageReading>;
}

export interface UsageChangedEvent {
  "eventId": Decimal;
  "at": string;
  "provider": UsageProvider;
  "state": UsageState;
  "primary"?: UsageWindow;
  "secondary"?: UsageWindow;
  "source": UsageSource;
  "fetchedAt": string;
}

export type EditorLanguage = "plain_text" | "rust" | "swift" | "typescript" | "python" | "json" | "markdown" | "shell";

export type EditorContentSource = "buffer" | "disk" | "draft" | "history";

export type FileSearchScope = "content" | "path";

export type HighlightKind = "keyword" | "string" | "comment" | "number" | "type" | "function" | "property" | "operator" | "punctuation" | "constant" | "variable";

export type DiagnosticSeverity = "error" | "warning" | "information" | "hint";

export type FileSymbolKind = "function" | "type" | "method" | "property" | "variable" | "module";

export type DiffLineKind = "context" | "added" | "removed";

export type LspState = "starting" | "ready" | "restarting" | "unavailable";

export interface TextPosition {
  "line": number;
  "column": number;
}

export interface TextRange {
  "start": TextPosition;
  "end": TextPosition;
}

export interface TextEdit {
  "range": TextRange;
  "text": string;
}

export interface HighlightSpan {
  "startColumn": number;
  "endColumn": number;
  "kind": HighlightKind;
}

export interface EditorLine {
  "line": number;
  "text": string;
  "highlights": Array<HighlightSpan>;
}

export interface FileAuthor {
  "actor": Actor;
  "name": string;
}

export interface FileDocument {
  "fileId": Id;
  "paneId": Id;
  "workspaceId": Id;
  "path": string;
  "language": EditorLanguage;
  "revision": Decimal;
  "diskRevision": Decimal;
  "lineCount": number;
  "byteCount": Decimal;
  "dirty": boolean;
  "conflicted": boolean;
  "watching": boolean;
}

export interface FileOpenParams {
  "workspaceId": Id;
  "path": string;
  "line"?: number;
  "column"?: number;
  "idempotencyKey": Id;
}

export interface FileOpenResult {
  "file": FileDocument;
  "pane": Pane;
  "viewport": FileReadResult;
}

export interface FileReadParams {
  "fileId": Id;
  "revision"?: Decimal;
  "source"?: EditorContentSource;
  "versionId"?: Id;
  "fromLine": number;
  "lineCount": number;
}

export interface FileReadResult {
  "file": FileDocument;
  "source": EditorContentSource;
  "fromLine": number;
  "toLine": number;
  "lines": Array<EditorLine>;
}

export interface FileWriteParams {
  "fileId": Id;
  "baseRevision": Decimal;
  "edits": Array<TextEdit>;
  "author": FileAuthor;
  "idempotencyKey": Id;
}

export interface FileWriteResult {
  "file": FileDocument;
  "changedLines": Array<number>;
}

export interface FileWatchParams {
  "fileId": Id;
  "watch": boolean;
  "idempotencyKey": Id;
}

export interface FileWatchResult {
  "fileId": Id;
  "watching": boolean;
}

export interface FileVersion {
  "versionId": Id;
  "fileId": Id;
  "revision": Decimal;
  "createdAt": string;
  "author": FileAuthor;
  "byteCount": Decimal;
}

export interface FileHistoryParams {
  "fileId": Id;
  "cursor"?: Id;
  "limit": number;
}

export interface FileHistoryResult {
  "items": Array<FileVersion>;
  "nextCursor"?: Id;
}

export interface FileRevertParams {
  "fileId": Id;
  "versionId": Id;
  "baseRevision": Decimal;
  "idempotencyKey": Id;
}

export interface FileRevertResult {
  "file": FileDocument;
}

export interface FileSearchMatch {
  "workspaceId": Id;
  "path": string;
  "range": TextRange;
  "preview": string;
}

export interface FileSearchParams {
  "workspaceId": Id;
  "scope"?: FileSearchScope;
  "query": string;
  "replacement"?: string;
  "caseSensitive": boolean;
  "limit": number;
  "idempotencyKey": Id;
}

export interface FileSearchResult {
  "items": Array<FileSearchMatch>;
  "replaced": number;
}

export interface FileSymbol {
  "name": string;
  "kind": FileSymbolKind;
  "range": TextRange;
  "selectionRange": TextRange;
}

export interface FileSymbolsParams {
  "fileId": Id;
  "revision": Decimal;
}

export interface FileSymbolsResult {
  "items": Array<FileSymbol>;
}

export interface FileLocation {
  "workspaceId": Id;
  "path": string;
  "range": TextRange;
}

export interface FilePositionParams {
  "fileId": Id;
  "revision": Decimal;
  "position": TextPosition;
}

export interface FileDefinitionResult {
  "location"?: FileLocation;
}

export interface FileReferencesResult {
  "items": Array<FileLocation>;
}

export interface FileDiagnostic {
  "diagnosticId": Id;
  "range": TextRange;
  "severity": DiagnosticSeverity;
  "message": string;
  "source": string;
}

export interface FileDiagnosticsParams {
  "fileId": Id;
  "revision": Decimal;
}

export interface FileDiagnosticsResult {
  "items": Array<FileDiagnostic>;
}

export interface FileBlame {
  "line": number;
  "commit": string;
  "author": string;
  "summary": string;
}

export interface DiffLine {
  "kind": DiffLineKind;
  "oldLine"?: number;
  "newLine"?: number;
  "text": string;
  "author"?: FileAuthor;
}

export interface DiffHunk {
  "hunkId": Id;
  "oldStart": number;
  "oldLines": number;
  "newStart": number;
  "newLines": number;
  "lines": Array<DiffLine>;
}

export interface FileDiffParams {
  "fileId": Id;
  "revision": Decimal;
}

export interface FileDiffResult {
  "fileId": Id;
  "revision": Decimal;
  "hunks": Array<DiffHunk>;
  "blame": Array<FileBlame>;
}

export interface FileFormatParams {
  "fileId": Id;
  "baseRevision": Decimal;
  "idempotencyKey": Id;
}

export interface FileFormatResult {
  "file": FileDocument;
}

export interface EditorReviewParams {
  "fileId": Id;
  "hunkId": Id;
  "baseRevision": Decimal;
  "idempotencyKey": Id;
}

export interface EditorReviewResult {
  "file": FileDocument;
  "hunkId": Id;
}

export interface EditorReviewCommentParams {
  "fileId": Id;
  "range": TextRange;
  "body": string;
  "idempotencyKey": Id;
}

export interface EditorReviewCommentResult {
  "commentId": Id;
  "taskId"?: Id;
  "delivered": boolean;
}

export interface LspServerStatus {
  "language": EditorLanguage;
  "state": LspState;
  "generation": number;
  "detail"?: string;
}

export interface LspStatusParams {
  "workspaceId"?: Id;
  "language"?: EditorLanguage;
}

export interface LspStatusResult {
  "servers": Array<LspServerStatus>;
}

export interface FileChangedOnDiskEvent {
  "eventId": Decimal;
  "at": string;
  "fileId": Id;
  "diskRevision": Decimal;
  "changedLines": Array<number>;
}

export interface FileSavedEvent {
  "eventId": Decimal;
  "at": string;
  "file": FileDocument;
  "by": FileAuthor;
}

export interface FileConflictEvent {
  "eventId": Decimal;
  "at": string;
  "fileId": Id;
  "baseRevision": Decimal;
  "draftRevision": Decimal;
  "diskRevision": Decimal;
}

export interface LspDiagnosticsEvent {
  "eventId": Decimal;
  "at": string;
  "fileId": Id;
  "revision": Decimal;
  "items": Array<FileDiagnostic>;
}

export interface EditorAgentCursorEvent {
  "eventId": Decimal;
  "at": string;
  "fileId": Id;
  "paneId": Id;
  "name": string;
  "position": TextPosition;
}

export interface EditorReviewRequestedEvent {
  "eventId": Decimal;
  "at": string;
  "fileId": Id;
  "revision": Decimal;
  "paneId": Id;
  "hunks": Array<DiffHunk>;
}

export type ClientCapability = "web_host";

export type WebInteractionMode = "structured" | "human";

export type WebNavigationAction = "back" | "forward" | "reload" | "reload_ignoring_cache";

export type WebViewportPreset = "iphone_se" | "iphone_15_pro" | "ipad" | "responsive";

export type WebConsoleLevel = "debug" | "info" | "warning" | "error";

export type WebSessionState = "anonymous" | "permission_required" | "granted" | "incognito";

export type WebCommandKind = "navigate" | "snapshot" | "click" | "type" | "press" | "hover" | "select" | "scroll" | "wait" | "evaluate" | "screenshot" | "console" | "network" | "resize" | "close";

export interface WebViewport {
  "width": number;
  "height": number;
  "preset": WebViewportPreset;
  "zoomPercent": number;
}

export interface WebRect {
  "x": number;
  "y": number;
  "width": number;
  "height": number;
}

export interface WebSnapshotNode {
  "ref": Id;
  "role": string;
  "name": string;
  "state": string;
  "value"?: string;
  "rect": WebRect;
  "parentRef"?: Id;
}

export interface WebConsoleEntry {
  "sequence": Decimal;
  "level": WebConsoleLevel;
  "text": string;
  "source": string;
  "at": string;
}

export interface WebNetworkEntry {
  "sequence": Decimal;
  "method": string;
  "url": string;
  "status": number;
  "durationMs": number;
  "bytes": Decimal;
  "at": string;
}

export interface LocalServer {
  "port": number;
  "pid": number;
  "command": string;
  "cwd": string;
  "title"?: string;
}

export interface WebComputedStyles {
  "boxSizing": string;
  "color": string;
  "background": string;
  "font": string;
  "border": string;
  "borderRadius": string;
  "display": string;
  "position": string;
}

export interface WebAccessibility {
  "role": string;
  "name": string;
  "state": string;
}

export interface WebComponentSource {
  "framework": string;
  "component": string;
  "sourceFile"?: string;
  "line"?: number;
}

export interface WebPickedElement {
  "selector": string;
  "path": Array<string>;
  "outerHtml": string;
  "styles": WebComputedStyles;
  "accessibility": WebAccessibility;
  "viewportRect": WebRect;
  "pageRect": WebRect;
  "image": AttachmentRef;
  "component"?: WebComponentSource;
}

export interface WebElementPackage {
  "url": string;
  "title": string;
  "elements": Array<WebPickedElement>;
}

export interface WebOpenParams {
  "url": string;
  "workspaceId"?: Id;
  "newPane"?: boolean;
  "incognito"?: boolean;
  "mode"?: WebInteractionMode;
  "idempotencyKey": Id;
}

export interface WebOpenResult {
  "paneId": Id;
}

export interface WebNavigateParams {
  "paneId": Id;
  "url"?: string;
  "action"?: WebNavigationAction;
  "idempotencyKey": Id;
}

export interface WebNavigateResult {
  "paneId": Id;
  "accepted": boolean;
}

export interface WebSnapshotParams {
  "paneId": Id;
  "screenshot"?: boolean;
}

export interface WebSnapshotResult {
  "paneId": Id;
  "navigationGeneration": Decimal;
  "title": string;
  "url": string;
  "nodes": Array<WebSnapshotNode>;
  "truncated": boolean;
  "screenshot"?: AttachmentRef;
}

export interface WebActionResult {
  "paneId": Id;
  "completed": boolean;
}

export interface WebClickParams {
  "paneId": Id;
  "ref": Id;
  "idempotencyKey": Id;
}

export interface WebTypeParams {
  "paneId": Id;
  "ref": Id;
  "text": string;
  "submit"?: boolean;
  "idempotencyKey": Id;
}

export interface WebPressParams {
  "paneId": Id;
  "key": string;
  "idempotencyKey": Id;
}

export interface WebHoverParams {
  "paneId": Id;
  "ref": Id;
  "idempotencyKey": Id;
}

export interface WebSelectParams {
  "paneId": Id;
  "ref": Id;
  "value": string;
  "idempotencyKey": Id;
}

export interface WebScrollParams {
  "paneId": Id;
  "ref"?: Id;
  "dx": number;
  "dy": number;
  "idempotencyKey": Id;
}

export interface WebWaitParams {
  "paneId": Id;
  "text"?: string;
  "selector"?: string;
  "networkIdle"?: boolean;
  "timeoutMs": number;
}

export interface WebWaitResult {
  "paneId": Id;
  "matched": boolean;
}

export interface WebEvaluateParams {
  "paneId": Id;
  "js": string;
  "idempotencyKey": Id;
}

export interface WebEvaluateResult {
  "paneId": Id;
  "value": string;
}

export interface WebScreenshotParams {
  "paneId": Id;
  "fullPage"?: boolean;
}

export interface WebScreenshotResult {
  "paneId": Id;
  "image": AttachmentRef;
}

export interface WebConsoleParams {
  "paneId": Id;
  "since"?: Decimal;
}

export interface WebConsoleResult {
  "paneId": Id;
  "entries": Array<WebConsoleEntry>;
  "next": Decimal;
  "truncated": boolean;
}

export interface WebNetworkParams {
  "paneId": Id;
  "since"?: Decimal;
}

export interface WebNetworkResult {
  "paneId": Id;
  "entries": Array<WebNetworkEntry>;
  "next": Decimal;
  "truncated": boolean;
}

export interface WebResizeParams {
  "paneId": Id;
  "preset"?: WebViewportPreset;
  "width"?: number;
  "height"?: number;
  "rotate"?: boolean;
  "idempotencyKey": Id;
}

export interface WebResizeResult {
  "paneId": Id;
  "viewport": WebViewport;
}

export interface WebCloseParams {
  "paneId": Id;
  "idempotencyKey": Id;
}

export interface WebCloseResult {
  "paneId": Id;
}

export interface WebCommandParams {
  "paneId": Id;
  "command": WebCommandKind;
  "args": JsonValue;
  "timeoutMs": number;
  "operationKey": Id;
}

export interface WebCommandResult {
  "paneId": Id;
  "value"?: JsonValue;
  "cookiesPresent"?: boolean;
}

export interface ServersLocalListParams {

}

export interface ServersLocalListResult {
  "servers": Array<LocalServer>;
}

export interface WebHostNavigatedParams {
  "paneId": Id;
  "navigationGeneration": Decimal;
  "url": string;
  "title": string;
  "idempotencyKey": Id;
}

export interface WebHostNavigatedResult {
  "accepted": boolean;
}

export interface WebHostConsoleCountsParams {
  "paneId": Id;
  "warnings": number;
  "errors": number;
  "idempotencyKey": Id;
}

export interface WebHostConsoleCountsResult {
  "accepted": boolean;
}

export interface WebNavigatedEvent {
  "eventId": Decimal;
  "at": string;
  "paneId": Id;
  "navigationGeneration": Decimal;
  "url": string;
  "title": string;
}

export interface WebConsoleCountsEvent {
  "paneId": Id;
  "warnings": number;
  "errors": number;
}

export interface WebAgentDrivingEvent {
  "paneId": Id;
  "on": boolean;
  "domain"?: string;
}

export interface ServersLocalChangedEvent {
  "servers": Array<LocalServer>;
}

export type PermissionRequestKind = "agent" | "web_session" | "web_evaluate";

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
  "pane.role.set": { params: PaneRoleSetParams; result: PaneRoleSetResult };
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
  "pane.prompt": { params: PanePromptParams; result: PanePromptResult };
  "pane.queue.list": { params: PaneQueueListParams; result: PaneQueueListResult };
  "pane.queue.remove": { params: PaneQueueRemoveParams; result: PaneQueueRemoveResult };
  "pane.cancel": { params: PaneCancelParams; result: PaneCancelResult };
  "pane.permission.answer": { params: PanePermissionAnswerParams; result: PanePermissionAnswerResult };
  "pane.elicitation.answer": { params: PaneElicitationAnswerParams; result: PaneElicitationAnswerResult };
  "pane.config.set": { params: PaneConfigSetParams; result: PaneConfigSetResult };
  "pane.yolo": { params: PaneYoloParams; result: PaneYoloResult };
  "pane.auth.start": { params: PaneAuthStartParams; result: PaneAuthStartResult };
  "pane.transcript": { params: PaneTranscriptParams; result: PaneTranscriptResult };
  "pane.draft": { params: PaneDraftParams; result: PaneDraftResult };
  "harness.list": { params: HarnessListParams; result: HarnessListResult };
  "model.list": { params: ModelListParams; result: ModelListResult };
  "pane.queue.resume": { params: PaneQueueResumeParams; result: PaneQueueResumeResult };
  "task.create": { params: TaskCreateParams; result: TaskCreateResult };
  "task.next": { params: TaskNextParams; result: TaskNextResult };
  "task.show": { params: TaskShowParams; result: TaskShowResult };
  "task.list": { params: TaskListParams; result: TaskListResult };
  "task.comments": { params: TaskCommentsParams; result: TaskCommentsResult };
  "task.update": { params: TaskUpdateParams; result: TaskUpdateResult };
  "task.comment": { params: TaskCommentParams; result: TaskCommentResult };
  "task.subitem.add": { params: TaskSubitemAddParams; result: TaskSubitemAddResult };
  "task.subitem.done": { params: TaskSubitemDoneParams; result: TaskSubitemDoneResult };
  "task.block": { params: TaskBlockParams; result: TaskBlockResult };
  "task.release": { params: TaskReleaseParams; result: TaskReleaseResult };
  "task.request_review": { params: TaskRequestReviewParams; result: TaskRequestReviewResult };
  "task.complete": { params: TaskCompleteParams; result: TaskCompleteResult };
  "task.dispatch": { params: TaskDispatchParams; result: TaskDispatchResult };
  "task.unblock": { params: TaskUnblockParams; result: TaskUnblockResult };
  "task.move": { params: TaskMoveParams; result: TaskMoveResult };
  "task.worktree.clean": { params: TaskWorktreeCleanParams; result: TaskWorktreeCleanResult };
  "loop.set": { params: LoopSetParams; result: LoopSetResult };
  "usage.list": { params: UsageListParams; result: UsageListResult };
  "usage.refresh": { params: UsageRefreshParams; result: UsageRefreshResult };
  "file.open": { params: FileOpenParams; result: FileOpenResult };
  "file.read": { params: FileReadParams; result: FileReadResult };
  "file.write": { params: FileWriteParams; result: FileWriteResult };
  "file.watch": { params: FileWatchParams; result: FileWatchResult };
  "file.history": { params: FileHistoryParams; result: FileHistoryResult };
  "file.revert": { params: FileRevertParams; result: FileRevertResult };
  "file.search": { params: FileSearchParams; result: FileSearchResult };
  "file.symbols": { params: FileSymbolsParams; result: FileSymbolsResult };
  "file.definition": { params: FilePositionParams; result: FileDefinitionResult };
  "file.references": { params: FilePositionParams; result: FileReferencesResult };
  "file.diagnostics": { params: FileDiagnosticsParams; result: FileDiagnosticsResult };
  "file.diff": { params: FileDiffParams; result: FileDiffResult };
  "file.format": { params: FileFormatParams; result: FileFormatResult };
  "editor.review.accept": { params: EditorReviewParams; result: EditorReviewResult };
  "editor.review.reject": { params: EditorReviewParams; result: EditorReviewResult };
  "editor.review.comment": { params: EditorReviewCommentParams; result: EditorReviewCommentResult };
  "lsp.status": { params: LspStatusParams; result: LspStatusResult };
  "servers.local.list": { params: ServersLocalListParams; result: ServersLocalListResult };
  "web.open": { params: WebOpenParams; result: WebOpenResult };
  "web.navigate": { params: WebNavigateParams; result: WebNavigateResult };
  "web.snapshot": { params: WebSnapshotParams; result: WebSnapshotResult };
  "web.click": { params: WebClickParams; result: WebActionResult };
  "web.type": { params: WebTypeParams; result: WebActionResult };
  "web.press": { params: WebPressParams; result: WebActionResult };
  "web.hover": { params: WebHoverParams; result: WebActionResult };
  "web.select": { params: WebSelectParams; result: WebActionResult };
  "web.scroll": { params: WebScrollParams; result: WebActionResult };
  "web.wait": { params: WebWaitParams; result: WebWaitResult };
  "web.evaluate": { params: WebEvaluateParams; result: WebEvaluateResult };
  "web.screenshot": { params: WebScreenshotParams; result: WebScreenshotResult };
  "web.console": { params: WebConsoleParams; result: WebConsoleResult };
  "web.network": { params: WebNetworkParams; result: WebNetworkResult };
  "web.resize": { params: WebResizeParams; result: WebResizeResult };
  "web.close": { params: WebCloseParams; result: WebCloseResult };
  "web.command": { params: WebCommandParams; result: WebCommandResult };
  "web.host.navigated": { params: WebHostNavigatedParams; result: WebHostNavigatedResult };
  "web.host.console_counts": { params: WebHostConsoleCountsParams; result: WebHostConsoleCountsResult };
}

export interface Events {
  "auth.challenge": AuthChallengeEvent;
  "state.snapshot_part": StateSnapshotPartEvent;
  "workspace.created": WorkspaceCreatedEvent;
  "workspace.renamed": WorkspaceRenamedEvent;
  "workspace.closed": WorkspaceClosedEvent;
  "pane.created": PaneCreatedEvent;
  "pane.renamed": PaneRenamedEvent;
  "pane.closed": PaneClosedEvent;
  "pane.state_changed": PaneStateChangedEvent;
  "stream.replay_start": StreamReplayStartEvent;
  "stream.replay_done": StreamReplayDoneEvent;
  "stream.gap": StreamGapEvent;
  "device.pair_requested": DevicePairRequestedEvent;
  "device.paired": DevicePairedEvent;
  "device.revoked": DeviceRevokedEvent;
  "pair.result": PairResultEvent;
  "engine.updating": EngineUpdatingEvent;
  "engine.restarted": EngineRestartedEvent;
  "ptyd.reconnected": PtydReconnectedEvent;
  "ptyd.lost": PtydLostEvent;
  "pane.updated": PaneUpdatedEvent;
  "settings.changed": SettingsChangedEvent;
  "pane.session_started": PaneSessionStartedEvent;
  "pane.turn_ended": PaneTurnEndedEvent;
  "pane.queued": PaneQueuedEvent;
  "pane.permission_requested": PanePermissionRequestedEvent;
  "pane.permission_answered": PanePermissionAnsweredEvent;
  "pane.elicitation_requested": PaneElicitationRequestedEvent;
  "pane.elicitation_answered": PaneElicitationAnsweredEvent;
  "pane.config_changing": PaneConfigChangingEvent;
  "pane.config_changed": PaneConfigChangedEvent;
  "pane.config_failed": PaneConfigFailedEvent;
  "pane.session_lost": PaneSessionLostEvent;
  "pane.needs_login": PaneNeedsLoginEvent;
  "pane.auth_finished": PaneAuthFinishedEvent;
  "pane.update": PaneUpdateEvent;
  "attention.changed": AttentionChangedEvent;
  "pane.queue.paused": PaneQueueStateEvent;
  "pane.queue.resumed": PaneQueueStateEvent;
  "task.created": TaskCreatedEvent;
  "task.moved": TaskMovedEvent;
  "task.claimed": TaskClaimedEvent;
  "task.released": TaskReleasedEvent;
  "task.commented": TaskCommentedEvent;
  "task.subitem_changed": TaskSubitemChangedEvent;
  "task.blocked": TaskBlockedEvent;
  "task.unblocked": TaskUnblockedEvent;
  "task.phrase_changed": TaskPhraseChangedEvent;
  "task.updated": TaskUpdatedEvent;
  "task.attention": TaskAttentionEvent;
  "loop.tick": LoopTickEvent;
  "loop.stopped": LoopStoppedEvent;
  "usage.changed": UsageChangedEvent;
  "file.changed_on_disk": FileChangedOnDiskEvent;
  "file.saved": FileSavedEvent;
  "file.conflict": FileConflictEvent;
  "lsp.diagnostics": LspDiagnosticsEvent;
  "editor.agent_cursor": EditorAgentCursorEvent;
  "editor.review_requested": EditorReviewRequestedEvent;
  "servers.local.changed": ServersLocalChangedEvent;
  "web.navigated": WebNavigatedEvent;
  "web.console_counts": WebConsoleCountsEvent;
  "web.agent_driving": WebAgentDrivingEvent;
}
export type EventName = keyof Events;
export interface EventDescriptor {
  paramsType: string;
  durable: boolean;
}
export const eventRegistry = {
  "auth.challenge": { paramsType: "AuthChallengeEvent", durable: false },
  "state.snapshot_part": { paramsType: "StateSnapshotPartEvent", durable: false },
  "workspace.created": { paramsType: "WorkspaceCreatedEvent", durable: true },
  "workspace.renamed": { paramsType: "WorkspaceRenamedEvent", durable: true },
  "workspace.closed": { paramsType: "WorkspaceClosedEvent", durable: true },
  "pane.created": { paramsType: "PaneCreatedEvent", durable: true },
  "pane.renamed": { paramsType: "PaneRenamedEvent", durable: true },
  "pane.closed": { paramsType: "PaneClosedEvent", durable: true },
  "pane.state_changed": { paramsType: "PaneStateChangedEvent", durable: true },
  "stream.replay_start": { paramsType: "StreamReplayStartEvent", durable: false },
  "stream.replay_done": { paramsType: "StreamReplayDoneEvent", durable: false },
  "stream.gap": { paramsType: "StreamGapEvent", durable: false },
  "device.pair_requested": { paramsType: "DevicePairRequestedEvent", durable: true },
  "device.paired": { paramsType: "DevicePairedEvent", durable: true },
  "device.revoked": { paramsType: "DeviceRevokedEvent", durable: true },
  "pair.result": { paramsType: "PairResultEvent", durable: false },
  "engine.updating": { paramsType: "EngineUpdatingEvent", durable: true },
  "engine.restarted": { paramsType: "EngineRestartedEvent", durable: true },
  "ptyd.reconnected": { paramsType: "PtydReconnectedEvent", durable: true },
  "ptyd.lost": { paramsType: "PtydLostEvent", durable: true },
  "pane.updated": { paramsType: "PaneUpdatedEvent", durable: true },
  "settings.changed": { paramsType: "SettingsChangedEvent", durable: true },
  "pane.session_started": { paramsType: "PaneSessionStartedEvent", durable: true },
  "pane.turn_ended": { paramsType: "PaneTurnEndedEvent", durable: true },
  "pane.queued": { paramsType: "PaneQueuedEvent", durable: true },
  "pane.permission_requested": { paramsType: "PanePermissionRequestedEvent", durable: true },
  "pane.permission_answered": { paramsType: "PanePermissionAnsweredEvent", durable: true },
  "pane.elicitation_requested": { paramsType: "PaneElicitationRequestedEvent", durable: true },
  "pane.elicitation_answered": { paramsType: "PaneElicitationAnsweredEvent", durable: true },
  "pane.config_changing": { paramsType: "PaneConfigChangingEvent", durable: true },
  "pane.config_changed": { paramsType: "PaneConfigChangedEvent", durable: true },
  "pane.config_failed": { paramsType: "PaneConfigFailedEvent", durable: true },
  "pane.session_lost": { paramsType: "PaneSessionLostEvent", durable: true },
  "pane.needs_login": { paramsType: "PaneNeedsLoginEvent", durable: true },
  "pane.auth_finished": { paramsType: "PaneAuthFinishedEvent", durable: true },
  "pane.update": { paramsType: "PaneUpdateEvent", durable: true },
  "attention.changed": { paramsType: "AttentionChangedEvent", durable: true },
  "pane.queue.paused": { paramsType: "PaneQueueStateEvent", durable: true },
  "pane.queue.resumed": { paramsType: "PaneQueueStateEvent", durable: true },
  "task.created": { paramsType: "TaskCreatedEvent", durable: true },
  "task.moved": { paramsType: "TaskMovedEvent", durable: true },
  "task.claimed": { paramsType: "TaskClaimedEvent", durable: true },
  "task.released": { paramsType: "TaskReleasedEvent", durable: true },
  "task.commented": { paramsType: "TaskCommentedEvent", durable: true },
  "task.subitem_changed": { paramsType: "TaskSubitemChangedEvent", durable: true },
  "task.blocked": { paramsType: "TaskBlockedEvent", durable: true },
  "task.unblocked": { paramsType: "TaskUnblockedEvent", durable: true },
  "task.phrase_changed": { paramsType: "TaskPhraseChangedEvent", durable: true },
  "task.updated": { paramsType: "TaskUpdatedEvent", durable: true },
  "task.attention": { paramsType: "TaskAttentionEvent", durable: true },
  "loop.tick": { paramsType: "LoopTickEvent", durable: true },
  "loop.stopped": { paramsType: "LoopStoppedEvent", durable: true },
  "usage.changed": { paramsType: "UsageChangedEvent", durable: true },
  "file.changed_on_disk": { paramsType: "FileChangedOnDiskEvent", durable: true },
  "file.saved": { paramsType: "FileSavedEvent", durable: true },
  "file.conflict": { paramsType: "FileConflictEvent", durable: true },
  "lsp.diagnostics": { paramsType: "LspDiagnosticsEvent", durable: true },
  "editor.agent_cursor": { paramsType: "EditorAgentCursorEvent", durable: true },
  "editor.review_requested": { paramsType: "EditorReviewRequestedEvent", durable: true },
  "servers.local.changed": { paramsType: "ServersLocalChangedEvent", durable: false },
  "web.navigated": { paramsType: "WebNavigatedEvent", durable: true },
  "web.console_counts": { paramsType: "WebConsoleCountsEvent", durable: false },
  "web.agent_driving": { paramsType: "WebAgentDrivingEvent", durable: false },
} as const satisfies Record<EventName, EventDescriptor>;
