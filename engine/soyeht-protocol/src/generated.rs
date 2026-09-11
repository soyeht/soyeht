// Generated from contracts/protocol.json (sha256 ea6797d079f3760fb7b2214734ee44e9842ee06f159f0419483b517b0f0cd8b3). Do not edit.
pub type JsonValue = serde_json::Value;
pub type Id = String;
pub type Decimal = String;
pub type ByteString = String;
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Role {
    #[serde(rename = "human_local")]
    HumanLocal,
    #[serde(rename = "human_device")]
    HumanDevice,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "chief")]
    Chief,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PaneMode {
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "agent")]
    Agent,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PaneState {
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "waiting_for_you")]
    WaitingForYou,
    #[serde(rename = "cancelling")]
    Cancelling,
    #[serde(rename = "no_signal")]
    NoSignal,
    #[serde(rename = "recovering")]
    Recovering,
    #[serde(rename = "dead")]
    Dead,
    #[serde(rename = "unknown")]
    Unknown,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StateSource {
    #[serde(rename = "protocol")]
    Protocol,
    #[serde(rename = "hook")]
    Hook,
    #[serde(rename = "osc")]
    Osc,
    #[serde(rename = "screen")]
    Screen,
    #[serde(rename = "none")]
    None,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SnapshotSection {
    #[serde(rename = "workspaces")]
    Workspaces,
    #[serde(rename = "panes")]
    Panes,
    #[serde(rename = "tasks")]
    Tasks,
    #[serde(rename = "requests")]
    Requests,
    #[serde(rename = "attention")]
    Attention,
    #[serde(rename = "devices")]
    Devices,
    #[serde(rename = "capabilities")]
    Capabilities,
    #[serde(rename = "streams")]
    Streams,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ErrorKind {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "invalid_params")]
    InvalidParams,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    #[serde(rename = "forbidden_role")]
    ForbiddenRole,
    #[serde(rename = "pane_busy")]
    PaneBusy,
    #[serde(rename = "task_has_owner")]
    TaskHasOwner,
    #[serde(rename = "transition_refused")]
    TransitionRefused,
    #[serde(rename = "stale_version")]
    StaleVersion,
    #[serde(rename = "idempotency_conflict")]
    IdempotencyConflict,
    #[serde(rename = "harness_unavailable")]
    HarnessUnavailable,
    #[serde(rename = "needs_login")]
    NeedsLogin,
    #[serde(rename = "cursor_too_old")]
    CursorTooOld,
    #[serde(rename = "resync_required")]
    ResyncRequired,
    #[serde(rename = "slow_consumer")]
    SlowConsumer,
    #[serde(rename = "protocol_version_unsupported")]
    ProtocolVersionUnsupported,
    #[serde(rename = "disk_full")]
    DiskFull,
    #[serde(rename = "ptyd_unavailable")]
    PtydUnavailable,
    #[serde(rename = "stale_engine_generation")]
    StaleEngineGeneration,
    #[serde(rename = "internal_error")]
    InternalError,
    #[serde(rename = "snapshot_too_large")]
    SnapshotTooLarge,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpcError {
    #[serde(rename = "code")]
    pub code: i64,
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "data")]
    pub data: ErrorData,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ErrorData {
    #[serde(rename = "kind")]
    pub kind: ErrorKind,
    #[serde(rename = "detail", skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Actor {
    #[serde(rename = "role")]
    pub role: Role,
    #[serde(rename = "client")]
    pub client: String,
    #[serde(rename = "deviceId", skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Id>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "layout")]
    pub layout: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pane {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "cwd")]
    pub cwd: String,
    #[serde(rename = "mode")]
    pub mode: PaneMode,
    #[serde(rename = "state")]
    pub state: PaneState,
    #[serde(rename = "source")]
    pub source: StateSource,
    #[serde(rename = "confidence")]
    pub confidence: f64,
    #[serde(rename = "phrase")]
    pub phrase: String,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "cols")]
    pub cols: u32,
    #[serde(rename = "rows")]
    pub rows: u32,
    #[serde(rename = "record")]
    pub record: bool,
    #[serde(rename = "pid", skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(rename = "streamId", skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<Id>,
    #[serde(rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSession>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Device {
    #[serde(rename = "deviceId")]
    pub device_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "fingerprint")]
    pub fingerprint: String,
    #[serde(rename = "revoked")]
    pub revoked: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamInfo {
    #[serde(rename = "streamId")]
    pub stream_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "firstRetainedOffset")]
    pub first_retained_offset: Decimal,
    #[serde(rename = "nextOffset")]
    pub next_offset: Decimal,
    #[serde(rename = "binaryPaneRef")]
    pub binary_pane_ref: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtydStatus {
    #[serde(rename = "connected")]
    pub connected: bool,
    #[serde(rename = "version")]
    pub version: u32,
    #[serde(rename = "generation")]
    pub generation: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DbStatus {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamCursor {
    #[serde(rename = "streamId")]
    pub stream_id: Id,
    #[serde(rename = "offset")]
    pub offset: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ReplayFrom {
    V0(TailSelector),
    V1(StreamCursor),
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Welcome {
    #[serde(rename = "engineVersion")]
    pub engine_version: String,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "connectionId")]
    pub connection_id: Id,
    #[serde(rename = "role")]
    pub role: Role,
    #[serde(rename = "capabilities")]
    pub capabilities: Vec<String>,
    #[serde(rename = "dbId")]
    pub db_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Empty {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HelloParams {
    #[serde(rename = "client")]
    pub client: String,
    #[serde(rename = "clientVersion")]
    pub client_version: String,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: u32,
    #[serde(rename = "deviceToken", skip_serializing_if = "Option::is_none")]
    pub device_token: Option<String>,
    #[serde(rename = "proof", skip_serializing_if = "Option::is_none")]
    pub proof: Option<ByteString>,
    #[serde(rename = "binding", skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StateSnapshotParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StateSnapshotResult {
    #[serde(rename = "snapshotId")]
    pub snapshot_id: Id,
    #[serde(rename = "cursor")]
    pub cursor: Decimal,
    #[serde(rename = "parts")]
    pub parts: u32,
    #[serde(rename = "focus", skip_serializing_if = "Option::is_none")]
    pub focus: Option<ClientFocus>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventsSubscribeParams {
    #[serde(rename = "since")]
    pub since: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventsSubscribeResult {
    #[serde(rename = "subscriptionId")]
    pub subscription_id: Id,
    #[serde(rename = "through")]
    pub through: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceListResult {
    #[serde(rename = "items")]
    pub items: Vec<Workspace>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCreateParams {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCreateResult {
    #[serde(rename = "workspace")]
    pub workspace: Workspace,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRenameParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRenameResult {
    #[serde(rename = "workspace")]
    pub workspace: Workspace,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCloseParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCloseResult {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceFocusParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceFocusResult {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCreateParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "mode")]
    pub mode: PaneMode,
    #[serde(rename = "cwd", skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "cols", skip_serializing_if = "Option::is_none")]
    pub cols: Option<u32>,
    #[serde(rename = "rows", skip_serializing_if = "Option::is_none")]
    pub rows: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
    #[serde(rename = "harness", skip_serializing_if = "Option::is_none")]
    pub harness: Option<Id>,
    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(rename = "effort", skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(rename = "permission", skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionMode>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCreateResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCloseParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCloseResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneFocusParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneFocusResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRenameParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRenameResult {
    #[serde(rename = "pane")]
    pub pane: Pane,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneResizeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "cols")]
    pub cols: u32,
    #[serde(rename = "rows")]
    pub rows: u32,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneResizeResult {
    #[serde(rename = "cols")]
    pub cols: u32,
    #[serde(rename = "rows")]
    pub rows: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneInputParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "bytes")]
    pub bytes: ByteString,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneInputResult {
    #[serde(rename = "acceptedBytes")]
    pub accepted_bytes: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneStreamSubscribeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "from")]
    pub from: ReplayFrom,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneStreamUnsubscribeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRecordSetParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "on")]
    pub on: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRecordSetResult {
    #[serde(rename = "on")]
    pub on: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairStartParams {
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairStartResult {
    #[serde(rename = "code")]
    pub code: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "uri")]
    pub uri: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairDecideParams {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "approve")]
    pub approve: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairDecideResult {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "approved")]
    pub approved: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceListResult {
    #[serde(rename = "items")]
    pub items: Vec<Device>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceRevokeParams {
    #[serde(rename = "deviceId")]
    pub device_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceRevokeResult {
    #[serde(rename = "deviceId")]
    pub device_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairBeginParams {
    #[serde(rename = "code")]
    pub code: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "devicePublicKey")]
    pub device_public_key: ByteString,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairBeginResult {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "challenge")]
    pub challenge: ByteString,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairProveParams {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "proof")]
    pub proof: ByteString,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairProveResult {
    #[serde(rename = "status")]
    pub status: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineStatusParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineStatusResult {
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "ptyd")]
    pub ptyd: PtydStatus,
    #[serde(rename = "db")]
    pub db: DbStatus,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsGetParams {
    #[serde(rename = "key")]
    pub key: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsGetResult {
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsSetParams {
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: JsonValue,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsSetResult {
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthChallengeEvent {
    #[serde(rename = "nonce")]
    pub nonce: ByteString,
    #[serde(rename = "engineId")]
    pub engine_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StateSnapshotPartEvent {
    #[serde(rename = "snapshotId")]
    pub snapshot_id: Id,
    #[serde(rename = "index")]
    pub index: u32,
    #[serde(rename = "of")]
    pub of: u32,
    #[serde(rename = "section")]
    pub section: SnapshotSection,
    #[serde(rename = "data")]
    pub data: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceCreatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspace")]
    pub workspace: Workspace,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRenamedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspace")]
    pub workspace: Workspace,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceClosedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCreatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "pane")]
    pub pane: Pane,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRenamedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "pane")]
    pub pane: Pane,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneClosedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneStateChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "state")]
    pub state: PaneState,
    #[serde(rename = "confidence")]
    pub confidence: f64,
    #[serde(rename = "phrase")]
    pub phrase: String,
    #[serde(rename = "source")]
    pub source: StateSource,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamReplayStartEvent {
    #[serde(rename = "streamId")]
    pub stream_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "fromOffset")]
    pub from_offset: Decimal,
    #[serde(rename = "toOffset")]
    pub to_offset: Decimal,
    #[serde(rename = "reset")]
    pub reset: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamReplayDoneEvent {
    #[serde(rename = "streamId")]
    pub stream_id: Id,
    #[serde(rename = "nextOffset")]
    pub next_offset: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StreamGapEvent {
    #[serde(rename = "streamId")]
    pub stream_id: Id,
    #[serde(rename = "fromOffset")]
    pub from_offset: Decimal,
    #[serde(rename = "toOffset")]
    pub to_offset: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairRequestedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "fingerprint")]
    pub fingerprint: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "device")]
    pub device: Device,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceRevokedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "deviceId")]
    pub device_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairResultEvent {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "approved")]
    pub approved: bool,
    #[serde(rename = "deviceToken", skip_serializing_if = "Option::is_none")]
    pub device_token: Option<String>,
    #[serde(rename = "deviceId", skip_serializing_if = "Option::is_none")]
    pub device_id: Option<Id>,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineUpdatingEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "generation")]
    pub generation: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EngineRestartedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "generation")]
    pub generation: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtydReconnectedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "generation")]
    pub generation: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtydLostEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TailSelector {
    #[serde(rename = "tail")]
    Tail,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RpcVersion {
    #[serde(rename = "2.0")]
    V20,
}
pub type RpcStringId = String;
pub type RpcNumericId = i64;
pub type RpcNullId = ();
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    V0(RpcStringId),
    V1(RpcNumericId),
    V2(RpcNullId),
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequestEnvelope {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: RpcVersion,
    #[serde(rename = "id")]
    pub id: RpcId,
    #[serde(rename = "method")]
    pub method: String,
    #[serde(rename = "params")]
    pub params: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NotificationEnvelope {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: RpcVersion,
    #[serde(rename = "method")]
    pub method: String,
    #[serde(rename = "params")]
    pub params: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SuccessEnvelope {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: RpcVersion,
    #[serde(rename = "id")]
    pub id: RpcId,
    #[serde(rename = "result")]
    pub result: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ErrorEnvelope {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: RpcVersion,
    #[serde(rename = "id")]
    pub id: RpcId,
    #[serde(rename = "error")]
    pub error: RpcError,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneUpdatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "pane")]
    pub pane: Pane,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SettingsChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: JsonValue,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DevicePairPending {
    #[serde(rename = "kind")]
    pub kind: String,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "deviceName")]
    pub device_name: String,
    #[serde(rename = "fingerprint")]
    pub fingerprint: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClientFocus {
    #[serde(rename = "workspaceId", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Id>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "ask")]
    Ask,
    #[serde(rename = "accept_edits")]
    AcceptEdits,
    #[serde(rename = "allow_all")]
    AllowAll,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PromptOrigin {
    #[serde(rename = "human")]
    Human,
    #[serde(rename = "automatic")]
    Automatic,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PromptStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "cancelled")]
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TranscriptRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "system")]
    System,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TranscriptKind {
    #[serde(rename = "message")]
    Message,
    #[serde(rename = "thought")]
    Thought,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "usage")]
    Usage,
    #[serde(rename = "system")]
    System,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionOptionKind {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "allow_always")]
    AllowAlways,
    #[serde(rename = "reject_once")]
    RejectOnce,
    #[serde(rename = "reject_always")]
    RejectAlways,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionResult {
    #[serde(rename = "selected")]
    Selected,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "expired")]
    Expired,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ElicitationResult {
    #[serde(rename = "submitted")]
    Submitted,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "expired")]
    Expired,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DecisionBy {
    #[serde(rename = "mac")]
    Mac,
    #[serde(rename = "iphone")]
    Iphone,
    #[serde(rename = "rule")]
    Rule,
    #[serde(rename = "system")]
    System,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProbeResult {
    #[serde(rename = "worked")]
    Worked,
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "inconclusive")]
    Inconclusive,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConfigType {
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "boolean")]
    Boolean,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ToolStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlanStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlanPriority {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "low")]
    Low,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SessionRecovery {
    #[serde(rename = "resumed")]
    Resumed,
    #[serde(rename = "new_session")]
    NewSession,
    #[serde(rename = "needs_login")]
    NeedsLogin,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConfigOperationKind {
    #[serde(rename = "config")]
    Config,
    #[serde(rename = "yolo")]
    Yolo,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    V0(String),
    V1(bool),
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigChoice {
    #[serde(rename = "value")]
    pub value: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigOption {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "category", skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub r#type: ConfigType,
    #[serde(rename = "currentValue")]
    pub current_value: ConfigValue,
    #[serde(rename = "options")]
    pub options: Vec<ConfigChoice>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigOperation {
    #[serde(rename = "operationId")]
    pub operation_id: Id,
    #[serde(rename = "kind")]
    pub kind: ConfigOperationKind,
    #[serde(rename = "configId")]
    pub config_id: Id,
    #[serde(rename = "value")]
    pub value: ConfigValue,
    #[serde(rename = "startedAt")]
    pub started_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptItem {
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "origin")]
    pub origin: PromptOrigin,
    #[serde(rename = "status")]
    pub status: PromptStatus,
    #[serde(rename = "queuedAt")]
    pub queued_at: String,
    #[serde(rename = "generation", skip_serializing_if = "Option::is_none")]
    pub generation: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionOption {
    #[serde(rename = "optionId")]
    pub option_id: Id,
    #[serde(rename = "kind")]
    pub kind: PermissionOptionKind,
    #[serde(rename = "name")]
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionRequest {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "detail")]
    pub detail: String,
    #[serde(rename = "options")]
    pub options: Vec<PermissionOption>,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ElicitationRequest {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "schema")]
    pub schema: JsonValue,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentSession {
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "fingerprint")]
    pub fingerprint: String,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Id>,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
    #[serde(rename = "authMethods")]
    pub auth_methods: Vec<AuthMethod>,
    #[serde(rename = "permission")]
    pub permission: PermissionMode,
    #[serde(rename = "yolo")]
    pub yolo: bool,
    #[serde(rename = "previousPermission", skip_serializing_if = "Option::is_none")]
    pub previous_permission: Option<PermissionMode>,
    #[serde(rename = "effectiveMode", skip_serializing_if = "Option::is_none")]
    pub effective_mode: Option<String>,
    #[serde(rename = "operation", skip_serializing_if = "Option::is_none")]
    pub operation: Option<ConfigOperation>,
    #[serde(rename = "queue")]
    pub queue: Vec<QueuedPromptSummary>,
    #[serde(rename = "draft")]
    pub draft: bool,
    #[serde(rename = "activePromptId", skip_serializing_if = "Option::is_none")]
    pub active_prompt_id: Option<Id>,
    #[serde(rename = "queuePaused", skip_serializing_if = "Option::is_none")]
    pub queue_paused: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProbeCapability {
    #[serde(rename = "method")]
    pub method: String,
    #[serde(rename = "result")]
    pub result: ProbeResult,
    #[serde(rename = "checkedAt")]
    pub checked_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthMethod {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "terminal")]
    pub terminal: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessInfo {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "available")]
    pub available: bool,
    #[serde(rename = "version", skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "fingerprint", skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(rename = "capabilities")]
    pub capabilities: Vec<ProbeCapability>,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
    #[serde(rename = "authMethods")]
    pub auth_methods: Vec<AuthMethod>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessLaunch {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "command")]
    pub command: String,
    #[serde(rename = "args")]
    pub args: Vec<String>,
    #[serde(rename = "version")]
    pub version: String,
}
pub type HarnessConfiguration = Vec<HarnessLaunch>;
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolLocation {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "line", skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "kind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(rename = "status")]
    pub status: ToolStatus,
    #[serde(rename = "content", skip_serializing_if = "Option::is_none")]
    pub content: Option<JsonValue>,
    #[serde(rename = "locations")]
    pub locations: Vec<ToolLocation>,
    #[serde(rename = "rawInput", skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<JsonValue>,
    #[serde(rename = "rawOutput", skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<JsonValue>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanEntry {
    #[serde(rename = "content")]
    pub content: String,
    #[serde(rename = "priority")]
    pub priority: PlanPriority,
    #[serde(rename = "status")]
    pub status: PlanStatus,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenUsage {
    #[serde(rename = "inputTokens", skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<Decimal>,
    #[serde(rename = "outputTokens", skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<Decimal>,
    #[serde(rename = "totalTokens")]
    pub total_tokens: Decimal,
    #[serde(rename = "contextSize", skip_serializing_if = "Option::is_none")]
    pub context_size: Option<Decimal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerminalOutput {
    #[serde(rename = "terminalId")]
    pub terminal_id: Id,
    #[serde(rename = "bytes")]
    pub bytes: ByteString,
    #[serde(rename = "offset")]
    pub offset: Decimal,
    #[serde(rename = "truncated")]
    pub truncated: bool,
    #[serde(rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TranscriptMessage {
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "role")]
    pub role: TranscriptRole,
    #[serde(rename = "kind")]
    pub kind: TranscriptKind,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "complete")]
    pub complete: bool,
    #[serde(rename = "promptId", skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<Id>,
    #[serde(rename = "tool", skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolCall>,
    #[serde(rename = "terminal", skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalOutput>,
    #[serde(rename = "plan", skip_serializing_if = "Option::is_none")]
    pub plan: Option<Vec<PlanEntry>>,
    #[serde(rename = "usage", skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttachmentRef {
    #[serde(rename = "attachmentId")]
    pub attachment_id: Id,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "bytes")]
    pub bytes: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePromptParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "origin", skip_serializing_if = "Option::is_none")]
    pub origin: Option<PromptOrigin>,
    #[serde(rename = "attachments", skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AttachmentRef>>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePromptResult {
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "queued")]
    pub queued: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueListParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
    #[serde(rename = "limit", skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueListResult {
    #[serde(rename = "items")]
    pub items: Vec<PromptItem>,
    #[serde(rename = "draft")]
    pub draft: bool,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
    #[serde(rename = "paused", skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueRemoveParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueRemoveResult {
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "removed")]
    pub removed: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCancelParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneCancelResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "promptId", skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<Id>,
    #[serde(rename = "cancelling")]
    pub cancelling: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePermissionAnswerParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "optionId")]
    pub option_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePermissionAnswerResult {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "result")]
    pub result: PermissionResult,
    #[serde(rename = "optionId", skip_serializing_if = "Option::is_none")]
    pub option_id: Option<Id>,
    #[serde(rename = "by")]
    pub by: DecisionBy,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneElicitationAnswerParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "fields", skip_serializing_if = "Option::is_none")]
    pub fields: Option<JsonValue>,
    #[serde(rename = "cancel", skip_serializing_if = "Option::is_none")]
    pub cancel: Option<bool>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneElicitationAnswerResult {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "result")]
    pub result: ElicitationResult,
    #[serde(rename = "by")]
    pub by: DecisionBy,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneConfigSetParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "configId")]
    pub config_id: Id,
    #[serde(rename = "value")]
    pub value: ConfigValue,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneConfigSetResult {
    #[serde(rename = "applied")]
    pub applied: bool,
    #[serde(rename = "effective")]
    pub effective: ConfigValue,
    #[serde(rename = "sessionPreserved")]
    pub session_preserved: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneYoloParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "on")]
    pub on: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneYoloResult {
    #[serde(rename = "effectiveMode")]
    pub effective_mode: PermissionMode,
    #[serde(rename = "on")]
    pub on: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneAuthStartParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "authMethodId", skip_serializing_if = "Option::is_none")]
    pub auth_method_id: Option<Id>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneAuthStartResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "terminalPaneId", skip_serializing_if = "Option::is_none")]
    pub terminal_pane_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneTranscriptParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "before", skip_serializing_if = "Option::is_none")]
    pub before: Option<Decimal>,
    #[serde(rename = "limit", skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneTranscriptResult {
    #[serde(rename = "items")]
    pub items: Vec<TranscriptMessage>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Decimal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneDraftParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "active")]
    pub active: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneDraftResult {
    #[serde(rename = "active")]
    pub active: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessListParams {
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
    #[serde(rename = "limit", skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessListResult {
    #[serde(rename = "items")]
    pub items: Vec<HarnessInfo>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelListParams {
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
    #[serde(rename = "limit", skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelListResult {
    #[serde(rename = "items")]
    pub items: Vec<ModelInfo>,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
    #[serde(rename = "source")]
    pub source: String,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneSessionStartedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "sessionId")]
    pub session_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneTurnEndedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "stopReason")]
    pub stop_reason: TurnStopReason,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueuedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "n")]
    pub n: u32,
    #[serde(rename = "promptIds")]
    pub prompt_ids: Vec<Id>,
    #[serde(rename = "draft")]
    pub draft: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePermissionRequestedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "detail")]
    pub detail: String,
    #[serde(rename = "options")]
    pub options: Vec<PermissionOption>,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanePermissionAnsweredEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "result")]
    pub result: PermissionResult,
    #[serde(rename = "optionId", skip_serializing_if = "Option::is_none")]
    pub option_id: Option<Id>,
    #[serde(rename = "by")]
    pub by: DecisionBy,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneElicitationRequestedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "schema")]
    pub schema: JsonValue,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneElicitationAnsweredEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "result")]
    pub result: ElicitationResult,
    #[serde(rename = "by")]
    pub by: DecisionBy,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneConfigChangingEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "configId")]
    pub config_id: Id,
    #[serde(rename = "value")]
    pub value: ConfigValue,
    #[serde(rename = "operationId")]
    pub operation_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneConfigChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "configId")]
    pub config_id: Id,
    #[serde(rename = "value")]
    pub value: ConfigValue,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
    #[serde(rename = "sessionPreserved")]
    pub session_preserved: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneConfigFailedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "configId")]
    pub config_id: Id,
    #[serde(rename = "value")]
    pub value: ConfigValue,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneSessionLostEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "recovery")]
    pub recovery: SessionRecovery,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneNeedsLoginEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "authMethods")]
    pub auth_methods: Vec<AuthMethod>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneAuthFinishedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ok")]
    pub ok: bool,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentMessageUpdateKind {
    #[serde(rename = "agent_message_chunk")]
    AgentMessageChunk,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentMessageUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: AgentMessageUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "complete")]
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentThoughtUpdateKind {
    #[serde(rename = "agent_thought_chunk")]
    AgentThoughtChunk,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentThoughtUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: AgentThoughtUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "complete")]
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ToolCallUpdateKind {
    #[serde(rename = "tool_call")]
    ToolCall,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolCallUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: ToolCallUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "tool")]
    pub tool: ToolCall,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ToolProgressUpdateKind {
    #[serde(rename = "tool_call_update")]
    ToolCallUpdate,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolProgressUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: ToolProgressUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "tool")]
    pub tool: ToolCall,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlanUpdateKind {
    #[serde(rename = "plan")]
    Plan,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: PlanUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "entries")]
    pub entries: Vec<PlanEntry>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UsageUpdateKind {
    #[serde(rename = "usage")]
    Usage,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: UsageUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "usage")]
    pub usage: TokenUsage,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TerminalOutputUpdateKind {
    #[serde(rename = "terminal_output")]
    TerminalOutput,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerminalOutputUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: TerminalOutputUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "terminalId")]
    pub terminal_id: Id,
    #[serde(rename = "bytes")]
    pub bytes: ByteString,
    #[serde(rename = "offset")]
    pub offset: Decimal,
    #[serde(rename = "truncated")]
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConfigOptionUpdateKind {
    #[serde(rename = "config_option_update")]
    ConfigOptionUpdate,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigOptionUpdate {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: ConfigOptionUpdateKind,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "seq")]
    pub seq: Decimal,
    #[serde(rename = "localId")]
    pub local_id: Id,
    #[serde(rename = "options")]
    pub options: Vec<ConfigOption>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PaneUpdateEvent {
    V0(AgentMessageUpdate),
    V1(AgentThoughtUpdate),
    V2(ToolCallUpdate),
    V3(ToolProgressUpdate),
    V4(PlanUpdate),
    V5(UsageUpdate),
    V6(TerminalOutputUpdate),
    V7(ConfigOptionUpdate),
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttentionItem {
    #[serde(rename = "attentionId")]
    pub attention_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Id>,
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttentionChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "count")]
    pub count: u32,
    #[serde(rename = "items")]
    pub items: Vec<AttentionItem>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TurnStopReason {
    #[serde(rename = "end_turn")]
    EndTurn,
    #[serde(rename = "max_tokens")]
    MaxTokens,
    #[serde(rename = "max_turn_requests")]
    MaxTurnRequests,
    #[serde(rename = "refusal")]
    Refusal,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "error")]
    Error,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueuedPromptSummary {
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "origin")]
    pub origin: PromptOrigin,
    #[serde(rename = "status")]
    pub status: PromptStatus,
    #[serde(rename = "queuedAt")]
    pub queued_at: String,
    #[serde(rename = "generation", skip_serializing_if = "Option::is_none")]
    pub generation: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueResumeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueResumeResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "paused")]
    pub paused: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneQueueStateEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "paused")]
    pub paused: bool,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    #[serde(rename = "hello")]
    Hello(HelloParams),
    #[serde(rename = "state.snapshot")]
    StateSnapshot(StateSnapshotParams),
    #[serde(rename = "events.subscribe")]
    EventsSubscribe(EventsSubscribeParams),
    #[serde(rename = "workspace.list")]
    WorkspaceList(WorkspaceListParams),
    #[serde(rename = "workspace.create")]
    WorkspaceCreate(WorkspaceCreateParams),
    #[serde(rename = "workspace.rename")]
    WorkspaceRename(WorkspaceRenameParams),
    #[serde(rename = "workspace.close")]
    WorkspaceClose(WorkspaceCloseParams),
    #[serde(rename = "workspace.focus")]
    WorkspaceFocus(WorkspaceFocusParams),
    #[serde(rename = "pane.create")]
    PaneCreate(PaneCreateParams),
    #[serde(rename = "pane.close")]
    PaneClose(PaneCloseParams),
    #[serde(rename = "pane.focus")]
    PaneFocus(PaneFocusParams),
    #[serde(rename = "pane.rename")]
    PaneRename(PaneRenameParams),
    #[serde(rename = "pane.resize")]
    PaneResize(PaneResizeParams),
    #[serde(rename = "pane.input")]
    PaneInput(PaneInputParams),
    #[serde(rename = "pane.stream.subscribe")]
    PaneStreamSubscribe(PaneStreamSubscribeParams),
    #[serde(rename = "pane.stream.unsubscribe")]
    PaneStreamUnsubscribe(PaneStreamUnsubscribeParams),
    #[serde(rename = "pane.record.set")]
    PaneRecordSet(PaneRecordSetParams),
    #[serde(rename = "device.pair.start")]
    DevicePairStart(DevicePairStartParams),
    #[serde(rename = "device.pair.decide")]
    DevicePairDecide(DevicePairDecideParams),
    #[serde(rename = "device.list")]
    DeviceList(DeviceListParams),
    #[serde(rename = "device.revoke")]
    DeviceRevoke(DeviceRevokeParams),
    #[serde(rename = "pair.begin")]
    PairBegin(PairBeginParams),
    #[serde(rename = "pair.prove")]
    PairProve(PairProveParams),
    #[serde(rename = "engine.status")]
    EngineStatus(EngineStatusParams),
    #[serde(rename = "settings.get")]
    SettingsGet(SettingsGetParams),
    #[serde(rename = "settings.set")]
    SettingsSet(SettingsSetParams),
    #[serde(rename = "pane.prompt")]
    PanePrompt(PanePromptParams),
    #[serde(rename = "pane.queue.list")]
    PaneQueueList(PaneQueueListParams),
    #[serde(rename = "pane.queue.remove")]
    PaneQueueRemove(PaneQueueRemoveParams),
    #[serde(rename = "pane.cancel")]
    PaneCancel(PaneCancelParams),
    #[serde(rename = "pane.permission.answer")]
    PanePermissionAnswer(PanePermissionAnswerParams),
    #[serde(rename = "pane.elicitation.answer")]
    PaneElicitationAnswer(PaneElicitationAnswerParams),
    #[serde(rename = "pane.config.set")]
    PaneConfigSet(PaneConfigSetParams),
    #[serde(rename = "pane.yolo")]
    PaneYolo(PaneYoloParams),
    #[serde(rename = "pane.auth.start")]
    PaneAuthStart(PaneAuthStartParams),
    #[serde(rename = "pane.transcript")]
    PaneTranscript(PaneTranscriptParams),
    #[serde(rename = "pane.draft")]
    PaneDraft(PaneDraftParams),
    #[serde(rename = "harness.list")]
    HarnessList(HarnessListParams),
    #[serde(rename = "model.list")]
    ModelList(ModelListParams),
    #[serde(rename = "pane.queue.resume")]
    PaneQueueResume(PaneQueueResumeParams),
}
pub const METHODS: &[(&str, &str, &str, bool)] = &[
    ("hello", "HelloParams", "Welcome", false),
    (
        "state.snapshot",
        "StateSnapshotParams",
        "StateSnapshotResult",
        false,
    ),
    (
        "events.subscribe",
        "EventsSubscribeParams",
        "EventsSubscribeResult",
        false,
    ),
    (
        "workspace.list",
        "WorkspaceListParams",
        "WorkspaceListResult",
        false,
    ),
    (
        "workspace.create",
        "WorkspaceCreateParams",
        "WorkspaceCreateResult",
        true,
    ),
    (
        "workspace.rename",
        "WorkspaceRenameParams",
        "WorkspaceRenameResult",
        true,
    ),
    (
        "workspace.close",
        "WorkspaceCloseParams",
        "WorkspaceCloseResult",
        true,
    ),
    (
        "workspace.focus",
        "WorkspaceFocusParams",
        "WorkspaceFocusResult",
        true,
    ),
    ("pane.create", "PaneCreateParams", "PaneCreateResult", true),
    ("pane.close", "PaneCloseParams", "PaneCloseResult", true),
    ("pane.focus", "PaneFocusParams", "PaneFocusResult", true),
    ("pane.rename", "PaneRenameParams", "PaneRenameResult", true),
    ("pane.resize", "PaneResizeParams", "PaneResizeResult", true),
    ("pane.input", "PaneInputParams", "PaneInputResult", true),
    (
        "pane.stream.subscribe",
        "PaneStreamSubscribeParams",
        "StreamInfo",
        false,
    ),
    (
        "pane.stream.unsubscribe",
        "PaneStreamUnsubscribeParams",
        "Empty",
        false,
    ),
    (
        "pane.record.set",
        "PaneRecordSetParams",
        "PaneRecordSetResult",
        true,
    ),
    (
        "device.pair.start",
        "DevicePairStartParams",
        "DevicePairStartResult",
        true,
    ),
    (
        "device.pair.decide",
        "DevicePairDecideParams",
        "DevicePairDecideResult",
        true,
    ),
    ("device.list", "DeviceListParams", "DeviceListResult", false),
    (
        "device.revoke",
        "DeviceRevokeParams",
        "DeviceRevokeResult",
        true,
    ),
    ("pair.begin", "PairBeginParams", "PairBeginResult", true),
    ("pair.prove", "PairProveParams", "PairProveResult", true),
    (
        "engine.status",
        "EngineStatusParams",
        "EngineStatusResult",
        false,
    ),
    (
        "settings.get",
        "SettingsGetParams",
        "SettingsGetResult",
        false,
    ),
    (
        "settings.set",
        "SettingsSetParams",
        "SettingsSetResult",
        true,
    ),
    ("pane.prompt", "PanePromptParams", "PanePromptResult", true),
    (
        "pane.queue.list",
        "PaneQueueListParams",
        "PaneQueueListResult",
        false,
    ),
    (
        "pane.queue.remove",
        "PaneQueueRemoveParams",
        "PaneQueueRemoveResult",
        true,
    ),
    ("pane.cancel", "PaneCancelParams", "PaneCancelResult", true),
    (
        "pane.permission.answer",
        "PanePermissionAnswerParams",
        "PanePermissionAnswerResult",
        true,
    ),
    (
        "pane.elicitation.answer",
        "PaneElicitationAnswerParams",
        "PaneElicitationAnswerResult",
        true,
    ),
    (
        "pane.config.set",
        "PaneConfigSetParams",
        "PaneConfigSetResult",
        true,
    ),
    ("pane.yolo", "PaneYoloParams", "PaneYoloResult", true),
    (
        "pane.auth.start",
        "PaneAuthStartParams",
        "PaneAuthStartResult",
        true,
    ),
    (
        "pane.transcript",
        "PaneTranscriptParams",
        "PaneTranscriptResult",
        false,
    ),
    ("pane.draft", "PaneDraftParams", "PaneDraftResult", true),
    (
        "harness.list",
        "HarnessListParams",
        "HarnessListResult",
        false,
    ),
    ("model.list", "ModelListParams", "ModelListResult", false),
    (
        "pane.queue.resume",
        "PaneQueueResumeParams",
        "PaneQueueResumeResult",
        true,
    ),
];

pub fn error_code(kind: &ErrorKind) -> i64 {
    match kind {
        ErrorKind::NotFound => -32000,
        ErrorKind::InvalidParams => -32001,
        ErrorKind::Unauthorized => -32002,
        ErrorKind::ForbiddenRole => -32003,
        ErrorKind::PaneBusy => -32004,
        ErrorKind::TaskHasOwner => -32005,
        ErrorKind::TransitionRefused => -32006,
        ErrorKind::StaleVersion => -32007,
        ErrorKind::IdempotencyConflict => -32008,
        ErrorKind::HarnessUnavailable => -32009,
        ErrorKind::NeedsLogin => -32010,
        ErrorKind::CursorTooOld => -32011,
        ErrorKind::ResyncRequired => -32012,
        ErrorKind::SlowConsumer => -32013,
        ErrorKind::ProtocolVersionUnsupported => -32014,
        ErrorKind::DiskFull => -32015,
        ErrorKind::PtydUnavailable => -32016,
        ErrorKind::StaleEngineGeneration => -32017,
        ErrorKind::InternalError => -32018,
        ErrorKind::SnapshotTooLarge => -32019,
    }
}
