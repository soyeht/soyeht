// Generated from contracts/protocol.json (sha256 b2528ce7f18d51677445f8aafc989b17a0a00c8002ae0fe860c1807b31fae56d). Do not edit.
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
    #[serde(rename = "iniciando")]
    Iniciando,
    #[serde(rename = "trabalhando")]
    Trabalhando,
    #[serde(rename = "ociosa")]
    Ociosa,
    #[serde(rename = "esperando_você")]
    EsperandoVocê,
    #[serde(rename = "cancelando")]
    Cancelando,
    #[serde(rename = "sem_sinal")]
    SemSinal,
    #[serde(rename = "recuperando")]
    Recuperando,
    #[serde(rename = "morta")]
    Morta,
    #[serde(rename = "não_sei")]
    NãoSei,
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
