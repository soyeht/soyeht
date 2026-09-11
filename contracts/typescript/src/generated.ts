// Generated from contracts/protocol.json (sha256 152ad52fe4b3c0b8fb0cfafaaad88dc1dcd804440536b6a0cb95ff8734017d90). Do not edit.
export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export type Id = string;

export type Decimal = string;

export type ByteString = string;

export type Role = "human_local" | "human_device" | "agent" | "chief";

export type PaneMode = "terminal" | "agent";

export type PaneState = "starting" | "working" | "idle" | "waiting_for_you" | "cancelling" | "no_signal" | "recovering" | "dead" | "unknown";

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
  "agent"?: AgentSession;
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
  "harness"?: Id;
  "model"?: string;
  "effort"?: string;
  "permission"?: PermissionMode;
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
  "permission": PermissionMode;
  "yolo": boolean;
  "previousPermission"?: PermissionMode;
  "effectiveMode"?: string;
  "operation"?: ConfigOperation;
  "queue": Array<PromptItem>;
  "draft": boolean;
  "activePromptId"?: Id;
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
}

export interface PaneQueueListResult {
  "items": Array<PromptItem>;
  "draft": boolean;
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

}

export interface HarnessListResult {
  "items": Array<HarnessInfo>;
}

export interface ModelListParams {
  "harness": Id;
  "paneId"?: Id;
}

export interface ModelListResult {
  "items": Array<ModelInfo>;
  "options": Array<ConfigOption>;
  "source": string;
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
}
