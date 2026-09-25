// Generated from contracts/protocol.json (sha256 575f970f590876a0567458ed0ed29a93577e81f9974b930c2f70c832b9c8e2c6). Do not edit.
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
pub enum AgentPaneRole {
    #[serde(rename = "worker")]
    Worker,
    #[serde(rename = "chief")]
    Chief,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PaneMode {
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "editor")]
    Editor,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "writing")]
    Writing,
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
    #[serde(rename = "loops")]
    Loops,
    #[serde(rename = "devices")]
    Devices,
    #[serde(rename = "capabilities")]
    Capabilities,
    #[serde(rename = "streams")]
    Streams,
    #[serde(rename = "files")]
    Files,
    #[serde(rename = "writing")]
    Writing,
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
    #[serde(rename = "engine_identity_changed")]
    EngineIdentityChanged,
    #[serde(rename = "stale_attempt")]
    StaleAttempt,
    #[serde(rename = "dependency_cycle")]
    DependencyCycle,
    #[serde(rename = "dependency_open")]
    DependencyOpen,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "stale_reference")]
    StaleReference,
    #[serde(rename = "human_took_over")]
    HumanTookOver,
    #[serde(rename = "web_host_unavailable")]
    WebHostUnavailable,
    #[serde(rename = "navigation_refused")]
    NavigationRefused,
    #[serde(rename = "writing_kind_refused")]
    WritingKindRefused,
    #[serde(rename = "writing_limit_exceeded")]
    WritingLimitExceeded,
    #[serde(rename = "writing_sidecar_conflict")]
    WritingSidecarConflict,
    #[serde(rename = "writing_line_not_found")]
    WritingLineNotFound,
    #[serde(rename = "writing_scene_not_found")]
    WritingSceneNotFound,
    #[serde(rename = "writing_section_refused")]
    WritingSectionRefused,
    #[serde(rename = "harness_switch_failed")]
    HarnessSwitchFailed,
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
    #[serde(rename = "retryAfterSec", skip_serializing_if = "Option::is_none")]
    pub retry_after_sec: Option<u32>,
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
pub enum WorkspacePaneLayoutMode {
    #[serde(rename = "automatic")]
    Automatic,
    #[serde(rename = "custom")]
    Custom,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WorkspacePaneLayoutKind {
    #[serde(rename = "terminal")]
    Terminal,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "editor")]
    Editor,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "presentation")]
    Presentation,
    #[serde(rename = "screenplay")]
    Screenplay,
    #[serde(rename = "unknown_writing")]
    UnknownWriting,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WorkspacePaneLayoutAxis {
    #[serde(rename = "horizontal")]
    Horizontal,
    #[serde(rename = "vertical")]
    Vertical,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspacePaneLayoutLeaf {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "kind")]
    pub kind: WorkspacePaneLayoutKind,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspacePaneLayoutBranch {
    #[serde(rename = "weight")]
    pub weight: f64,
    #[serde(rename = "node")]
    pub node: WorkspacePaneLayoutNode,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspacePaneLayoutCompanion {
    #[serde(rename = "writingPaneId")]
    pub writing_pane_id: Id,
    #[serde(rename = "agentPaneId")]
    pub agent_pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspacePaneLayoutSplit {
    #[serde(rename = "axis")]
    pub axis: WorkspacePaneLayoutAxis,
    #[serde(rename = "children")]
    pub children: Vec<WorkspacePaneLayoutBranch>,
    #[serde(rename = "companion", skip_serializing_if = "Option::is_none")]
    pub companion: Option<WorkspacePaneLayoutCompanion>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum WorkspacePaneLayoutNode {
    V0(WorkspacePaneLayoutLeaf),
    V1(WorkspacePaneLayoutSplit),
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspacePaneLayoutEnvelope {
    #[serde(rename = "version")]
    pub version: u32,
    #[serde(rename = "mode")]
    pub mode: WorkspacePaneLayoutMode,
    #[serde(rename = "tree", skip_serializing_if = "Option::is_none")]
    pub tree: Option<WorkspacePaneLayoutNode>,
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
    #[serde(rename = "experimentCapture", skip_serializing_if = "Option::is_none")]
    pub experiment_capture: Option<PaneExperimentCapture>,
    #[serde(rename = "pid", skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(rename = "streamId", skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<Id>,
    #[serde(rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentSession>,
    #[serde(rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "viewport", skip_serializing_if = "Option::is_none")]
    pub viewport: Option<WebViewport>,
    #[serde(rename = "incognito", skip_serializing_if = "Option::is_none")]
    pub incognito: Option<bool>,
    #[serde(rename = "webSession", skip_serializing_if = "Option::is_none")]
    pub web_session: Option<WebSessionState>,
    #[serde(rename = "writingKind", skip_serializing_if = "Option::is_none")]
    pub writing_kind: Option<WritingKind>,
    #[serde(rename = "writingSection", skip_serializing_if = "Option::is_none")]
    pub writing_section: Option<WritingSection>,
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
    #[serde(rename = "capabilities", skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<ClientCapability>>,
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
pub struct WorkspaceLayoutSetParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "layout")]
    pub layout: WorkspacePaneLayoutEnvelope,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceLayoutSetResult {
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
    #[serde(rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "incognito", skip_serializing_if = "Option::is_none")]
    pub incognito: Option<bool>,
    #[serde(rename = "path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(rename = "writingKind", skip_serializing_if = "Option::is_none")]
    pub writing_kind: Option<WritingKind>,
    #[serde(rename = "writingSection", skip_serializing_if = "Option::is_none")]
    pub writing_section: Option<WritingSection>,
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
pub struct PaneExperimentCapture {
    #[serde(rename = "on")]
    pub on: bool,
    #[serde(rename = "failure", skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentSetParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "on")]
    pub on: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentSetResult {
    #[serde(rename = "capture")]
    pub capture: PaneExperimentCapture,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentDeleteParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentDeleteResult {
    #[serde(rename = "files")]
    pub files: u32,
    #[serde(rename = "bytes")]
    pub bytes: Decimal,
    #[serde(rename = "nothingRemained")]
    pub nothing_remained: bool,
    #[serde(rename = "exportsPath", skip_serializing_if = "Option::is_none")]
    pub exports_path: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentExportParams {
    #[serde(rename = "paneIds")]
    pub pane_ids: Vec<Id>,
    #[serde(rename = "fromMs")]
    pub from_ms: Decimal,
    #[serde(rename = "toMs")]
    pub to_ms: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentSkippedSource {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "segments")]
    pub segments: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentExportResult {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "rows")]
    pub rows: u32,
    #[serde(rename = "bytes")]
    pub bytes: Decimal,
    #[serde(rename = "sha256")]
    pub sha256: String,
    #[serde(rename = "actualFromMs", skip_serializing_if = "Option::is_none")]
    pub actual_from_ms: Option<Decimal>,
    #[serde(rename = "actualToMs", skip_serializing_if = "Option::is_none")]
    pub actual_to_ms: Option<Decimal>,
    #[serde(
        rename = "staleTemporaryFilesRemoved",
        skip_serializing_if = "Option::is_none"
    )]
    pub stale_temporary_files_removed: Option<u32>,
    #[serde(
        rename = "staleTemporaryBytesRemoved",
        skip_serializing_if = "Option::is_none"
    )]
    pub stale_temporary_bytes_removed: Option<Decimal>,
    #[serde(rename = "skippedSources", skip_serializing_if = "Option::is_none")]
    pub skipped_sources: Option<Vec<PaneExperimentSkippedSource>>,
    #[serde(
        rename = "skippedSourceNotice",
        skip_serializing_if = "Option::is_none"
    )]
    pub skipped_source_notice: Option<String>,
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
pub struct WorkspaceLayoutUpdatedEvent {
    #[serde(rename = "workspace")]
    pub workspace: Workspace,
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "actor", skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
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
    #[serde(rename = "file", skip_serializing_if = "Option::is_none")]
    pub file: Option<FileDocument>,
    #[serde(rename = "writingDocument", skip_serializing_if = "Option::is_none")]
    pub writing_document: Option<WritingDocument>,
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
    #[serde(rename = "attachments", skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AttachmentRef>>,
    #[serde(rename = "agentMessage", skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<AgentMessageSource>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentMessageSource {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "harness")]
    pub harness: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentListResult {
    #[serde(rename = "items")]
    pub items: Vec<AgentMessageSource>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentMessageParams {
    #[serde(rename = "targetPaneId")]
    pub target_pane_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentMessageResult {
    #[serde(rename = "promptId")]
    pub prompt_id: Id,
    #[serde(rename = "queued")]
    pub queued: bool,
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
pub struct PermissionFileChange {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "oldText", skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    #[serde(rename = "newText")]
    pub new_text: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionPresentation {
    #[serde(rename = "targetPath", skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    #[serde(rename = "command", skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(rename = "executionDetails", skip_serializing_if = "Option::is_none")]
    pub execution_details: Option<Vec<String>>,
    #[serde(rename = "fileChanges")]
    pub file_changes: Vec<PermissionFileChange>,
    #[serde(rename = "previewOmitted")]
    pub preview_omitted: bool,
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
    #[serde(rename = "presentation", skip_serializing_if = "Option::is_none")]
    pub presentation: Option<PermissionPresentation>,
    #[serde(rename = "options")]
    pub options: Vec<PermissionOption>,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "kind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<PermissionRequestKind>,
    #[serde(rename = "domain", skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
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
pub struct AgentHarnessSwitch {
    #[serde(rename = "fromHarness")]
    pub from_harness: Id,
    #[serde(rename = "toHarness")]
    pub to_harness: Id,
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
    #[serde(rename = "silentSince", skip_serializing_if = "Option::is_none")]
    pub silent_since: Option<String>,
    #[serde(rename = "queuePaused", skip_serializing_if = "Option::is_none")]
    pub queue_paused: Option<bool>,
    #[serde(rename = "role")]
    pub role: AgentPaneRole,
    #[serde(rename = "harnessSwitch", skip_serializing_if = "Option::is_none")]
    pub harness_switch: Option<AgentHarnessSwitch>,
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
    #[serde(rename = "installed", skip_serializing_if = "Option::is_none")]
    pub installed: Option<bool>,
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
    #[serde(rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<HarnessSource>,
    #[serde(rename = "setupState", skip_serializing_if = "Option::is_none")]
    pub setup_state: Option<HarnessSetupState>,
    #[serde(rename = "setupReason", skip_serializing_if = "Option::is_none")]
    pub setup_reason: Option<HarnessSetupReason>,
    #[serde(rename = "lastReadyAtMs", skip_serializing_if = "Option::is_none")]
    pub last_ready_at_ms: Option<Decimal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelRefusal {
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "observedAt")]
    pub observed_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    #[serde(rename = "id")]
    pub id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "observedRefusal", skip_serializing_if = "Option::is_none")]
    pub observed_refusal: Option<ModelRefusal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ModelListSource {
    #[serde(rename = "not_probed")]
    NotProbed,
    #[serde(rename = "native_session")]
    NativeSession,
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
    #[serde(rename = "environment", skip_serializing_if = "Option::is_none")]
    pub environment: Option<Vec<HarnessEnvironmentVariable>>,
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
pub struct PaneHarnessSwitchParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(rename = "effort", skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneHarnessSwitchResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "fromHarness")]
    pub from_harness: Id,
    #[serde(rename = "toHarness")]
    pub to_harness: Id,
    #[serde(rename = "newSessionId")]
    pub new_session_id: Id,
    #[serde(rename = "markerSeq")]
    pub marker_seq: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneHarnessSwitchStartedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "fromHarness")]
    pub from_harness: Id,
    #[serde(rename = "toHarness")]
    pub to_harness: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneHarnessSwitchDoneEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "fromHarness")]
    pub from_harness: Id,
    #[serde(rename = "toHarness")]
    pub to_harness: Id,
    #[serde(rename = "newSessionId")]
    pub new_session_id: Id,
    #[serde(rename = "markerSeq")]
    pub marker_seq: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneHarnessSwitchFailedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "fromHarness")]
    pub from_harness: Id,
    #[serde(rename = "toHarness")]
    pub to_harness: Id,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "restored")]
    pub restored: bool,
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
    #[serde(rename = "rescan", skip_serializing_if = "Option::is_none")]
    pub rescan: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessListResult {
    #[serde(rename = "items")]
    pub items: Vec<HarnessInfo>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
    #[serde(rename = "environmentIssue", skip_serializing_if = "Option::is_none")]
    pub environment_issue: Option<HarnessEnvironmentIssue>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomListResult {
    #[serde(rename = "items")]
    pub items: Vec<HarnessLaunch>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomCreateParams {
    #[serde(rename = "harness")]
    pub harness: HarnessLaunch,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomUpdateParams {
    #[serde(rename = "harness")]
    pub harness: HarnessLaunch,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomMutationResult {
    #[serde(rename = "harness")]
    pub harness: HarnessLaunch,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomRemoveParams {
    #[serde(rename = "harnessId")]
    pub harness_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessCustomRemoveResult {
    #[serde(rename = "harnessId")]
    pub harness_id: Id,
    #[serde(rename = "removed")]
    pub removed: bool,
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
    pub source: ModelListSource,
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
    #[serde(rename = "presentation", skip_serializing_if = "Option::is_none")]
    pub presentation: Option<PermissionPresentation>,
    #[serde(rename = "options")]
    pub options: Vec<PermissionOption>,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
    #[serde(rename = "kind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<PermissionRequestKind>,
    #[serde(rename = "domain", skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
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
    #[serde(rename = "system_message")]
    SystemMessage,
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskState {
    #[serde(rename = "todo")]
    Todo,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "in_review")]
    InReview,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "cancelled")]
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskPriority {
    #[serde(rename = "urgent")]
    Urgent,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "low")]
    Low,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SubitemState {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "done")]
    Done,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AttemptResult {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "returned")]
    Returned,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlockingKind {
    #[serde(rename = "dependency")]
    Dependency,
    #[serde(rename = "missing_information")]
    MissingInformation,
    #[serde(rename = "external_error")]
    ExternalError,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskCheckState {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "green")]
    Green,
    #[serde(rename = "red")]
    Red,
    #[serde(rename = "conflict")]
    Conflict,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskReviewState {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "awaiting_person")]
    AwaitingPerson,
    #[serde(rename = "changes_requested")]
    ChangesRequested,
    #[serde(rename = "approved")]
    Approved,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaskReviewBadge {
    #[serde(rename = "system_turn")]
    SystemTurn,
    #[serde(rename = "your_turn")]
    YourTurn,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LoopStopWhen {
    #[serde(rename = "round_limit")]
    RoundLimit,
    #[serde(rename = "three_empty")]
    ThreeEmpty,
    #[serde(rename = "explicit_off")]
    ExplicitOff,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DriftLabel {
    #[serde(rename = "equivalent")]
    Equivalent,
    #[serde(rename = "style")]
    Style,
    #[serde(rename = "algorithm")]
    Algorithm,
    #[serde(rename = "input_gating")]
    InputGating,
    #[serde(rename = "silent_fallback")]
    SilentFallback,
    #[serde(rename = "error_handling")]
    ErrorHandling,
    #[serde(rename = "return_shape")]
    ReturnShape,
    #[serde(rename = "external_effects")]
    ExternalEffects,
    #[serde(rename = "extra_edits")]
    ExtraEdits,
    #[serde(rename = "incomplete")]
    Incomplete,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskExecutor {
    #[serde(rename = "workspaceDefault")]
    pub workspace_default: bool,
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
pub struct TaskWorktree {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "branch")]
    pub branch: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskPullRequest {
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "checkState")]
    pub check_state: TaskCheckState,
    #[serde(rename = "reviewState")]
    pub review_state: TaskReviewState,
    #[serde(rename = "head", skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Blocking {
    #[serde(rename = "kind")]
    pub kind: BlockingKind,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "dependencyTaskId", skip_serializing_if = "Option::is_none")]
    pub dependency_task_id: Option<Id>,
    #[serde(rename = "retryCount", skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Subitem {
    #[serde(rename = "subitemId")]
    pub subitem_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<Id>,
    #[serde(rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(rename = "state")]
    pub state: SubitemState,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    #[serde(rename = "commentId")]
    pub comment_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "at")]
    pub at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Attempt {
    #[serde(rename = "n")]
    pub n: u32,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "harness")]
    pub harness: Id,
    #[serde(rename = "model")]
    pub model: String,
    #[serde(rename = "start")]
    pub start: String,
    #[serde(rename = "end", skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(rename = "result", skip_serializing_if = "Option::is_none")]
    pub result: Option<AttemptResult>,
    #[serde(rename = "summary", skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskMove {
    #[serde(rename = "actor")]
    pub actor: String,
    #[serde(rename = "from")]
    pub from: TaskState,
    #[serde(rename = "to")]
    pub to: TaskState,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "at")]
    pub at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContractChange {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "label")]
    pub label: DriftLabel,
    #[serde(rename = "contractRef")]
    pub contract_ref: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutOfScopeEdit {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoopConfig {
    #[serde(rename = "prompt")]
    pub prompt: String,
    #[serde(rename = "interval")]
    pub interval: u32,
    #[serde(rename = "maxRounds")]
    pub max_rounds: u32,
    #[serde(rename = "stopWhen")]
    pub stop_when: LoopStopWhen,
    #[serde(rename = "enabled")]
    pub enabled: bool,
    #[serde(rename = "rounds", skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
    #[serde(rename = "emptyResults", skip_serializing_if = "Option::is_none")]
    pub empty_results: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Task {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "description")]
    pub description: String,
    #[serde(rename = "howToVerify", skip_serializing_if = "Option::is_none")]
    pub how_to_verify: Option<String>,
    #[serde(rename = "mustNotChange")]
    pub must_not_change: String,
    #[serde(rename = "state")]
    pub state: TaskState,
    #[serde(rename = "priority")]
    pub priority: TaskPriority,
    #[serde(rename = "plannedOwner", skip_serializing_if = "Option::is_none")]
    pub planned_owner: Option<Id>,
    #[serde(rename = "ownerPaneId", skip_serializing_if = "Option::is_none")]
    pub owner_pane_id: Option<Id>,
    #[serde(rename = "executor", skip_serializing_if = "Option::is_none")]
    pub executor: Option<TaskExecutor>,
    #[serde(rename = "dependencies")]
    pub dependencies: Vec<String>,
    #[serde(rename = "worktree", skip_serializing_if = "Option::is_none")]
    pub worktree: Option<TaskWorktree>,
    #[serde(rename = "currentAttempt")]
    pub current_attempt: u32,
    #[serde(rename = "blocking", skip_serializing_if = "Option::is_none")]
    pub blocking: Option<Blocking>,
    #[serde(rename = "pr", skip_serializing_if = "Option::is_none")]
    pub pr: Option<TaskPullRequest>,
    #[serde(rename = "prContract")]
    pub pr_contract: bool,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "createdBy")]
    pub created_by: String,
    #[serde(rename = "version")]
    pub version: u32,
    #[serde(rename = "phrase")]
    pub phrase: String,
    #[serde(rename = "reviewBadge", skip_serializing_if = "Option::is_none")]
    pub review_badge: Option<TaskReviewBadge>,
    #[serde(rename = "summary", skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "contractChanges", skip_serializing_if = "Option::is_none")]
    pub contract_changes: Option<Vec<ContractChange>>,
    #[serde(rename = "outOfScopeEdits", skip_serializing_if = "Option::is_none")]
    pub out_of_scope_edits: Option<Vec<OutOfScopeEdit>>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCreateParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "howToVerify", skip_serializing_if = "Option::is_none")]
    pub how_to_verify: Option<String>,
    #[serde(rename = "mustNotChange")]
    pub must_not_change: String,
    #[serde(rename = "executor", skip_serializing_if = "Option::is_none")]
    pub executor: Option<TaskExecutor>,
    #[serde(rename = "priority", skip_serializing_if = "Option::is_none")]
    pub priority: Option<TaskPriority>,
    #[serde(rename = "createWorktree", skip_serializing_if = "Option::is_none")]
    pub create_worktree: Option<bool>,
    #[serde(rename = "permission", skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionMode>,
    #[serde(rename = "dependencies", skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(rename = "dispatch", skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<bool>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCreateResult {
    #[serde(rename = "task")]
    pub task: Task,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskNextParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "sessionGeneration")]
    pub session_generation: u32,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskNextResult {
    #[serde(rename = "task", skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
    #[serde(rename = "empty")]
    pub empty: bool,
    #[serde(rename = "willRestartSession")]
    pub will_restart_session: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskShowParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskShowResult {
    #[serde(rename = "task")]
    pub task: Task,
    #[serde(rename = "subitems")]
    pub subitems: Vec<Subitem>,
    #[serde(rename = "comments")]
    pub comments: Vec<Comment>,
    #[serde(rename = "attempts")]
    pub attempts: Vec<Attempt>,
    #[serde(rename = "history")]
    pub history: Vec<TaskMove>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskListParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "state", skip_serializing_if = "Option::is_none")]
    pub state: Option<TaskState>,
    #[serde(rename = "limit")]
    pub limit: u32,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskListResult {
    #[serde(rename = "items")]
    pub items: Vec<Task>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCommentsParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "before", skip_serializing_if = "Option::is_none")]
    pub before: Option<Id>,
    #[serde(rename = "limit")]
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCommentsResult {
    #[serde(rename = "items")]
    pub items: Vec<Comment>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUpdateParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "version")]
    pub version: u32,
    #[serde(rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "howToVerify", skip_serializing_if = "Option::is_none")]
    pub how_to_verify: Option<String>,
    #[serde(rename = "mustNotChange", skip_serializing_if = "Option::is_none")]
    pub must_not_change: Option<String>,
    #[serde(rename = "priority", skip_serializing_if = "Option::is_none")]
    pub priority: Option<TaskPriority>,
    #[serde(rename = "dependencies", skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(rename = "plannedOwner", skip_serializing_if = "Option::is_none")]
    pub planned_owner: Option<Id>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUpdateResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCommentParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCommentResult {
    #[serde(rename = "comment")]
    pub comment: Comment,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSubitemAddParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<Id>,
    #[serde(rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSubitemAddResult {
    #[serde(rename = "subitem")]
    pub subitem: Subitem,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSubitemDoneParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "subitemId")]
    pub subitem_id: Id,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSubitemDoneResult {
    #[serde(rename = "subitem")]
    pub subitem: Subitem,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskBlockParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "kind")]
    pub kind: BlockingKind,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "dependencyTaskId", skip_serializing_if = "Option::is_none")]
    pub dependency_task_id: Option<Id>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskBlockResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskReleaseParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskReleaseResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskRequestReviewParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskRequestReviewResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCompleteParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "summary")]
    pub summary: String,
    #[serde(rename = "contractChanges")]
    pub contract_changes: Vec<ContractChange>,
    #[serde(rename = "outOfScopeEdits")]
    pub out_of_scope_edits: Vec<OutOfScopeEdit>,
    #[serde(rename = "paneId", skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<Id>,
    #[serde(rename = "sessionGeneration", skip_serializing_if = "Option::is_none")]
    pub session_generation: Option<u32>,
    #[serde(rename = "attempt", skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCompleteResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskDispatchParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "preset")]
    pub preset: TaskExecutor,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskDispatchResult {
    #[serde(rename = "task")]
    pub task: Task,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "attempt")]
    pub attempt: u32,
    #[serde(rename = "resumed")]
    pub resumed: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUnblockParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUnblockResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskMoveParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "state")]
    pub state: TaskState,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskMoveResult {
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskWorktreeCleanParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskWorktreeCleanResult {
    #[serde(rename = "cleaned")]
    pub cleaned: bool,
    #[serde(rename = "reason", skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoopSetParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "prompt")]
    pub prompt: String,
    #[serde(rename = "interval")]
    pub interval: u32,
    #[serde(rename = "maxRounds")]
    pub max_rounds: u32,
    #[serde(rename = "stopWhen")]
    pub stop_when: LoopStopWhen,
    #[serde(rename = "enabled")]
    pub enabled: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoopSetResult {
    #[serde(rename = "config")]
    pub config: LoopConfig,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCreatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "task")]
    pub task: Task,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskMovedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "actor")]
    pub actor: String,
    #[serde(rename = "from")]
    pub from: TaskState,
    #[serde(rename = "to")]
    pub to: TaskState,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskClaimedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "attempt")]
    pub attempt: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskReleasedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "reason")]
    pub reason: String,
    #[serde(rename = "attempt")]
    pub attempt: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskCommentedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "comment")]
    pub comment: Comment,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskSubitemChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "subitem")]
    pub subitem: Subitem,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskBlockedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "kind")]
    pub kind: BlockingKind,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUnblockedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskPhraseChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "phrase")]
    pub phrase: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUpdatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "version")]
    pub version: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskAttentionEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "taskId")]
    pub task_id: Id,
    #[serde(rename = "n")]
    pub n: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoopTickEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "round")]
    pub round: u32,
    #[serde(rename = "of")]
    pub of: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoopStoppedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "reason")]
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRoleSetParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "role")]
    pub role: AgentPaneRole,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneRoleSetResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "role")]
    pub role: AgentPaneRole,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UsageProvider {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UsageState {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "stale")]
    Stale,
    #[serde(rename = "login_needed")]
    LoginNeeded,
    #[serde(rename = "no_access")]
    NoAccess,
    #[serde(rename = "unavailable")]
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UsageUnavailableReason {
    #[serde(rename = "not_checked")]
    NotChecked,
    #[serde(rename = "tool_not_found")]
    ToolNotFound,
    #[serde(rename = "app_server_failed")]
    AppServerFailed,
    #[serde(rename = "invalid_response")]
    InvalidResponse,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UsageSource {
    #[serde(rename = "claude_cli_usage")]
    ClaudeCliUsage,
    #[serde(rename = "claude_oauth_endpoint")]
    ClaudeOauthEndpoint,
    #[serde(rename = "claude_desktop_cache")]
    ClaudeDesktopCache,
    #[serde(rename = "codex_app_server")]
    CodexAppServer,
    #[serde(rename = "codex_account_endpoint")]
    CodexAccountEndpoint,
    #[serde(rename = "cursor_current_period")]
    CursorCurrentPeriod,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageWindow {
    #[serde(rename = "used")]
    pub used: f64,
    #[serde(rename = "resetsAt")]
    pub resets_at: String,
    #[serde(rename = "label")]
    pub label: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageReading {
    #[serde(rename = "provider")]
    pub provider: UsageProvider,
    #[serde(rename = "state")]
    pub state: UsageState,
    #[serde(rename = "unavailableReason", skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<UsageUnavailableReason>,
    #[serde(rename = "primary", skip_serializing_if = "Option::is_none")]
    pub primary: Option<UsageWindow>,
    #[serde(rename = "secondary", skip_serializing_if = "Option::is_none")]
    pub secondary: Option<UsageWindow>,
    #[serde(rename = "source")]
    pub source: UsageSource,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageListResult {
    #[serde(rename = "providers")]
    pub providers: Vec<UsageReading>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRefreshParams {
    #[serde(rename = "provider", skip_serializing_if = "Option::is_none")]
    pub provider: Option<UsageProvider>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageRefreshResult {
    #[serde(rename = "providers")]
    pub providers: Vec<UsageReading>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "provider")]
    pub provider: UsageProvider,
    #[serde(rename = "state")]
    pub state: UsageState,
    #[serde(rename = "unavailableReason", skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<UsageUnavailableReason>,
    #[serde(rename = "primary", skip_serializing_if = "Option::is_none")]
    pub primary: Option<UsageWindow>,
    #[serde(rename = "secondary", skip_serializing_if = "Option::is_none")]
    pub secondary: Option<UsageWindow>,
    #[serde(rename = "source")]
    pub source: UsageSource,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EditorLanguage {
    #[serde(rename = "plain_text")]
    PlainText,
    #[serde(rename = "rust")]
    Rust,
    #[serde(rename = "swift")]
    Swift,
    #[serde(rename = "typescript")]
    Typescript,
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "markdown")]
    Markdown,
    #[serde(rename = "shell")]
    Shell,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EditorContentSource {
    #[serde(rename = "buffer")]
    Buffer,
    #[serde(rename = "disk")]
    Disk,
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "history")]
    History,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FileSearchScope {
    #[serde(rename = "content")]
    Content,
    #[serde(rename = "path")]
    Path,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HighlightKind {
    #[serde(rename = "keyword")]
    Keyword,
    #[serde(rename = "string")]
    String,
    #[serde(rename = "comment")]
    Comment,
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "type")]
    Type,
    #[serde(rename = "function")]
    Function,
    #[serde(rename = "property")]
    Property,
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "punctuation")]
    Punctuation,
    #[serde(rename = "constant")]
    Constant,
    #[serde(rename = "variable")]
    Variable,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticSeverity {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "information")]
    Information,
    #[serde(rename = "hint")]
    Hint,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FileSymbolKind {
    #[serde(rename = "function")]
    Function,
    #[serde(rename = "type")]
    Type,
    #[serde(rename = "method")]
    Method,
    #[serde(rename = "property")]
    Property,
    #[serde(rename = "variable")]
    Variable,
    #[serde(rename = "module")]
    Module,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DiffLineKind {
    #[serde(rename = "context")]
    Context,
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "removed")]
    Removed,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LspState {
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "restarting")]
    Restarting,
    #[serde(rename = "unavailable")]
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextPosition {
    #[serde(rename = "line")]
    pub line: u32,
    #[serde(rename = "column")]
    pub column: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextRange {
    #[serde(rename = "start")]
    pub start: TextPosition,
    #[serde(rename = "end")]
    pub end: TextPosition,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TextEdit {
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "text")]
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HighlightSpan {
    #[serde(rename = "startColumn")]
    pub start_column: u32,
    #[serde(rename = "endColumn")]
    pub end_column: u32,
    #[serde(rename = "kind")]
    pub kind: HighlightKind,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorLine {
    #[serde(rename = "line")]
    pub line: u32,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "highlights")]
    pub highlights: Vec<HighlightSpan>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileAuthor {
    #[serde(rename = "actor")]
    pub actor: Actor,
    #[serde(rename = "name")]
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDocument {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "language")]
    pub language: EditorLanguage,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "diskRevision")]
    pub disk_revision: Decimal,
    #[serde(rename = "lineCount")]
    pub line_count: u32,
    #[serde(rename = "byteCount")]
    pub byte_count: Decimal,
    #[serde(rename = "dirty")]
    pub dirty: bool,
    #[serde(rename = "conflicted")]
    pub conflicted: bool,
    #[serde(rename = "watching")]
    pub watching: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileOpenParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "line", skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(rename = "column", skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileOpenResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "pane")]
    pub pane: Pane,
    #[serde(rename = "viewport")]
    pub viewport: FileReadResult,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileReadParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision", skip_serializing_if = "Option::is_none")]
    pub revision: Option<Decimal>,
    #[serde(rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<EditorContentSource>,
    #[serde(rename = "versionId", skip_serializing_if = "Option::is_none")]
    pub version_id: Option<Id>,
    #[serde(rename = "fromLine")]
    pub from_line: u32,
    #[serde(rename = "lineCount")]
    pub line_count: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileReadResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "source")]
    pub source: EditorContentSource,
    #[serde(rename = "fromLine")]
    pub from_line: u32,
    #[serde(rename = "toLine")]
    pub to_line: u32,
    #[serde(rename = "lines")]
    pub lines: Vec<EditorLine>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileWriteParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "edits")]
    pub edits: Vec<TextEdit>,
    #[serde(rename = "author")]
    pub author: FileAuthor,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileWriteResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "changedLines")]
    pub changed_lines: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileWatchParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "watch")]
    pub watch: bool,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileWatchResult {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "watching")]
    pub watching: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileVersion {
    #[serde(rename = "versionId")]
    pub version_id: Id,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "author")]
    pub author: FileAuthor,
    #[serde(rename = "byteCount")]
    pub byte_count: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileHistoryParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
    #[serde(rename = "limit")]
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileHistoryResult {
    #[serde(rename = "items")]
    pub items: Vec<FileVersion>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileRevertParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "versionId")]
    pub version_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileRevertResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSearchMatch {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "preview")]
    pub preview: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSearchParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "scope", skip_serializing_if = "Option::is_none")]
    pub scope: Option<FileSearchScope>,
    #[serde(rename = "query")]
    pub query: String,
    #[serde(rename = "replacement", skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(rename = "caseSensitive")]
    pub case_sensitive: bool,
    #[serde(rename = "limit")]
    pub limit: u32,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSearchResult {
    #[serde(rename = "items")]
    pub items: Vec<FileSearchMatch>,
    #[serde(rename = "replaced")]
    pub replaced: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSymbol {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: FileSymbolKind,
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "selectionRange")]
    pub selection_range: TextRange,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSymbolsParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSymbolsResult {
    #[serde(rename = "items")]
    pub items: Vec<FileSymbol>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileLocation {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "range")]
    pub range: TextRange,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilePositionParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "position")]
    pub position: TextPosition,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDefinitionResult {
    #[serde(rename = "location", skip_serializing_if = "Option::is_none")]
    pub location: Option<FileLocation>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileReferencesResult {
    #[serde(rename = "items")]
    pub items: Vec<FileLocation>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiagnostic {
    #[serde(rename = "diagnosticId")]
    pub diagnostic_id: Id,
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "severity")]
    pub severity: DiagnosticSeverity,
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "source")]
    pub source: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiagnosticsParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiagnosticsResult {
    #[serde(rename = "items")]
    pub items: Vec<FileDiagnostic>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileBlame {
    #[serde(rename = "line")]
    pub line: u32,
    #[serde(rename = "commit")]
    pub commit: String,
    #[serde(rename = "author")]
    pub author: String,
    #[serde(rename = "summary")]
    pub summary: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DiffLine {
    #[serde(rename = "kind")]
    pub kind: DiffLineKind,
    #[serde(rename = "oldLine", skip_serializing_if = "Option::is_none")]
    pub old_line: Option<u32>,
    #[serde(rename = "newLine", skip_serializing_if = "Option::is_none")]
    pub new_line: Option<u32>,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "author", skip_serializing_if = "Option::is_none")]
    pub author: Option<FileAuthor>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DiffHunk {
    #[serde(rename = "hunkId")]
    pub hunk_id: Id,
    #[serde(rename = "oldStart")]
    pub old_start: u32,
    #[serde(rename = "oldLines")]
    pub old_lines: u32,
    #[serde(rename = "newStart")]
    pub new_start: u32,
    #[serde(rename = "newLines")]
    pub new_lines: u32,
    #[serde(rename = "lines")]
    pub lines: Vec<DiffLine>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiffParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiffResult {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "hunks")]
    pub hunks: Vec<DiffHunk>,
    #[serde(rename = "blame")]
    pub blame: Vec<FileBlame>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileFormatParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileFormatResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorReviewParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "hunkId")]
    pub hunk_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorReviewResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "hunkId")]
    pub hunk_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorReviewCommentParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "body")]
    pub body: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorReviewCommentResult {
    #[serde(rename = "commentId")]
    pub comment_id: Id,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Id>,
    #[serde(rename = "delivered")]
    pub delivered: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LspServerStatus {
    #[serde(rename = "language")]
    pub language: EditorLanguage,
    #[serde(rename = "state")]
    pub state: LspState,
    #[serde(rename = "generation")]
    pub generation: u32,
    #[serde(rename = "detail", skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LspStatusParams {
    #[serde(rename = "workspaceId", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Id>,
    #[serde(rename = "language", skip_serializing_if = "Option::is_none")]
    pub language: Option<EditorLanguage>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LspStatusResult {
    #[serde(rename = "servers")]
    pub servers: Vec<LspServerStatus>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileChangedOnDiskEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "diskRevision")]
    pub disk_revision: Decimal,
    #[serde(rename = "changedLines")]
    pub changed_lines: Vec<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSavedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "by")]
    pub by: FileAuthor,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileConflictEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "draftRevision")]
    pub draft_revision: Decimal,
    #[serde(rename = "diskRevision")]
    pub disk_revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LspDiagnosticsEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "items")]
    pub items: Vec<FileDiagnostic>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorAgentCursorEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "position")]
    pub position: TextPosition,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EditorReviewRequestedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "hunks")]
    pub hunks: Vec<DiffHunk>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ClientCapability {
    #[serde(rename = "web_host")]
    WebHost,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebInteractionMode {
    #[serde(rename = "structured")]
    Structured,
    #[serde(rename = "human")]
    Human,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebNavigationAction {
    #[serde(rename = "back")]
    Back,
    #[serde(rename = "forward")]
    Forward,
    #[serde(rename = "reload")]
    Reload,
    #[serde(rename = "reload_ignoring_cache")]
    ReloadIgnoringCache,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebViewportPreset {
    #[serde(rename = "iphone_se")]
    IphoneSe,
    #[serde(rename = "iphone_15_pro")]
    Iphone15Pro,
    #[serde(rename = "ipad")]
    Ipad,
    #[serde(rename = "responsive")]
    Responsive,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebConsoleLevel {
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebSessionState {
    #[serde(rename = "anonymous")]
    Anonymous,
    #[serde(rename = "permission_required")]
    PermissionRequired,
    #[serde(rename = "granted")]
    Granted,
    #[serde(rename = "incognito")]
    Incognito,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WebCommandKind {
    #[serde(rename = "navigate")]
    Navigate,
    #[serde(rename = "snapshot")]
    Snapshot,
    #[serde(rename = "click")]
    Click,
    #[serde(rename = "type")]
    Type,
    #[serde(rename = "press")]
    Press,
    #[serde(rename = "hover")]
    Hover,
    #[serde(rename = "select")]
    Select,
    #[serde(rename = "scroll")]
    Scroll,
    #[serde(rename = "wait")]
    Wait,
    #[serde(rename = "evaluate")]
    Evaluate,
    #[serde(rename = "screenshot")]
    Screenshot,
    #[serde(rename = "console")]
    Console,
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "resize")]
    Resize,
    #[serde(rename = "close")]
    Close,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebViewport {
    #[serde(rename = "width")]
    pub width: u32,
    #[serde(rename = "height")]
    pub height: u32,
    #[serde(rename = "preset")]
    pub preset: WebViewportPreset,
    #[serde(rename = "zoomPercent")]
    pub zoom_percent: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebRect {
    #[serde(rename = "x")]
    pub x: i64,
    #[serde(rename = "y")]
    pub y: i64,
    #[serde(rename = "width")]
    pub width: u32,
    #[serde(rename = "height")]
    pub height: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSnapshotNode {
    #[serde(rename = "ref")]
    pub r#ref: Id,
    #[serde(rename = "role")]
    pub role: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "state")]
    pub state: String,
    #[serde(rename = "value", skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "rect")]
    pub rect: WebRect,
    #[serde(rename = "parentRef", skip_serializing_if = "Option::is_none")]
    pub parent_ref: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebConsoleEntry {
    #[serde(rename = "sequence")]
    pub sequence: Decimal,
    #[serde(rename = "level")]
    pub level: WebConsoleLevel,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "source")]
    pub source: String,
    #[serde(rename = "at")]
    pub at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNetworkEntry {
    #[serde(rename = "sequence")]
    pub sequence: Decimal,
    #[serde(rename = "method")]
    pub method: String,
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "status")]
    pub status: u32,
    #[serde(rename = "durationMs")]
    pub duration_ms: u32,
    #[serde(rename = "bytes")]
    pub bytes: Decimal,
    #[serde(rename = "at")]
    pub at: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocalServer {
    #[serde(rename = "port")]
    pub port: u32,
    #[serde(rename = "pid")]
    pub pid: u32,
    #[serde(rename = "command")]
    pub command: String,
    #[serde(rename = "cwd")]
    pub cwd: String,
    #[serde(rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebComputedStyles {
    #[serde(rename = "boxSizing")]
    pub box_sizing: String,
    #[serde(rename = "color")]
    pub color: String,
    #[serde(rename = "background")]
    pub background: String,
    #[serde(rename = "font")]
    pub font: String,
    #[serde(rename = "border")]
    pub border: String,
    #[serde(rename = "borderRadius")]
    pub border_radius: String,
    #[serde(rename = "display")]
    pub display: String,
    #[serde(rename = "position")]
    pub position: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebAccessibility {
    #[serde(rename = "role")]
    pub role: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "state")]
    pub state: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebComponentSource {
    #[serde(rename = "framework")]
    pub framework: String,
    #[serde(rename = "component")]
    pub component: String,
    #[serde(rename = "sourceFile", skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(rename = "line", skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebPickedElement {
    #[serde(rename = "selector")]
    pub selector: String,
    #[serde(rename = "path")]
    pub path: Vec<String>,
    #[serde(rename = "outerHtml")]
    pub outer_html: String,
    #[serde(rename = "styles")]
    pub styles: WebComputedStyles,
    #[serde(rename = "accessibility")]
    pub accessibility: WebAccessibility,
    #[serde(rename = "viewportRect")]
    pub viewport_rect: WebRect,
    #[serde(rename = "pageRect")]
    pub page_rect: WebRect,
    #[serde(rename = "image")]
    pub image: AttachmentRef,
    #[serde(rename = "component", skip_serializing_if = "Option::is_none")]
    pub component: Option<WebComponentSource>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebElementPackage {
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "elements")]
    pub elements: Vec<WebPickedElement>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebOpenParams {
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "workspaceId", skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Id>,
    #[serde(rename = "newPane", skip_serializing_if = "Option::is_none")]
    pub new_pane: Option<bool>,
    #[serde(rename = "incognito", skip_serializing_if = "Option::is_none")]
    pub incognito: Option<bool>,
    #[serde(rename = "mode", skip_serializing_if = "Option::is_none")]
    pub mode: Option<WebInteractionMode>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebOpenResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNavigateParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "action", skip_serializing_if = "Option::is_none")]
    pub action: Option<WebNavigationAction>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNavigateResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "accepted")]
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSnapshotParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "screenshot", skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSnapshotResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "navigationGeneration")]
    pub navigation_generation: Decimal,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "nodes")]
    pub nodes: Vec<WebSnapshotNode>,
    #[serde(rename = "truncated")]
    pub truncated: bool,
    #[serde(rename = "screenshot", skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<AttachmentRef>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebActionResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "completed")]
    pub completed: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebClickParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ref")]
    pub r#ref: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebTypeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ref")]
    pub r#ref: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "submit", skip_serializing_if = "Option::is_none")]
    pub submit: Option<bool>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebPressParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebHoverParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ref")]
    pub r#ref: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSelectParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ref")]
    pub r#ref: Id,
    #[serde(rename = "value")]
    pub value: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebScrollParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<Id>,
    #[serde(rename = "dx")]
    pub dx: i64,
    #[serde(rename = "dy")]
    pub dy: i64,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebWaitParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "text", skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "selector", skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(rename = "networkIdle", skip_serializing_if = "Option::is_none")]
    pub network_idle: Option<bool>,
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebWaitResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "matched")]
    pub matched: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebEvaluateParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "js")]
    pub js: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebEvaluateResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "value")]
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebScreenshotParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "fullPage", skip_serializing_if = "Option::is_none")]
    pub full_page: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebScreenshotResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "image")]
    pub image: AttachmentRef,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebConsoleParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "since", skip_serializing_if = "Option::is_none")]
    pub since: Option<Decimal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebConsoleResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "entries")]
    pub entries: Vec<WebConsoleEntry>,
    #[serde(rename = "next")]
    pub next: Decimal,
    #[serde(rename = "truncated")]
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNetworkParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "since", skip_serializing_if = "Option::is_none")]
    pub since: Option<Decimal>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNetworkResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "entries")]
    pub entries: Vec<WebNetworkEntry>,
    #[serde(rename = "next")]
    pub next: Decimal,
    #[serde(rename = "truncated")]
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebResizeParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "preset", skip_serializing_if = "Option::is_none")]
    pub preset: Option<WebViewportPreset>,
    #[serde(rename = "width", skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(rename = "height", skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(rename = "rotate", skip_serializing_if = "Option::is_none")]
    pub rotate: Option<bool>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebResizeResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "viewport")]
    pub viewport: WebViewport,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebCloseParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebCloseResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebCommandParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "command")]
    pub command: WebCommandKind,
    #[serde(rename = "args")]
    pub args: JsonValue,
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u32,
    #[serde(rename = "operationKey")]
    pub operation_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebCommandResult {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "value", skip_serializing_if = "Option::is_none")]
    pub value: Option<JsonValue>,
    #[serde(rename = "cookiesPresent", skip_serializing_if = "Option::is_none")]
    pub cookies_present: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServersLocalListParams {}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServersLocalListResult {
    #[serde(rename = "servers")]
    pub servers: Vec<LocalServer>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebHostNavigatedParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "navigationGeneration")]
    pub navigation_generation: Decimal,
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebHostNavigatedResult {
    #[serde(rename = "accepted")]
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebHostConsoleCountsParams {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "warnings")]
    pub warnings: u32,
    #[serde(rename = "errors")]
    pub errors: u32,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebHostConsoleCountsResult {
    #[serde(rename = "accepted")]
    pub accepted: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebNavigatedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "navigationGeneration")]
    pub navigation_generation: Decimal,
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "title")]
    pub title: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebConsoleCountsEvent {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "warnings")]
    pub warnings: u32,
    #[serde(rename = "errors")]
    pub errors: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebAgentDrivingEvent {
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "on")]
    pub on: bool,
    #[serde(rename = "domain", skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServersLocalChangedEvent {
    #[serde(rename = "servers")]
    pub servers: Vec<LocalServer>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PermissionRequestKind {
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "web_session")]
    WebSession,
    #[serde(rename = "web_evaluate")]
    WebEvaluate,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WritingKind {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "presentation")]
    Presentation,
    #[serde(rename = "screenplay")]
    Screenplay,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WritingSection {
    #[serde(rename = "texts")]
    Texts,
    #[serde(rename = "presentations")]
    Presentations,
    #[serde(rename = "screenplays")]
    Screenplays,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WritingFont {
    #[serde(rename = "reading_serif")]
    ReadingSerif,
    #[serde(rename = "classic_serif")]
    ClassicSerif,
    #[serde(rename = "sans")]
    Sans,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WritingFormatKind {
    #[serde(rename = "heading_1")]
    Heading1,
    #[serde(rename = "heading_2")]
    Heading2,
    #[serde(rename = "heading_3")]
    Heading3,
    #[serde(rename = "bold")]
    Bold,
    #[serde(rename = "italic")]
    Italic,
    #[serde(rename = "bullet_list")]
    BulletList,
    #[serde(rename = "numbered_list")]
    NumberedList,
    #[serde(rename = "quote")]
    Quote,
    #[serde(rename = "link")]
    Link,
    #[serde(rename = "image")]
    Image,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ScreenplayElementKind {
    #[serde(rename = "scene_heading")]
    SceneHeading,
    #[serde(rename = "action")]
    Action,
    #[serde(rename = "character")]
    Character,
    #[serde(rename = "parenthetical")]
    Parenthetical,
    #[serde(rename = "dialogue")]
    Dialogue,
    #[serde(rename = "transition")]
    Transition,
    #[serde(rename = "shot")]
    Shot,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BeatCategory {
    #[serde(rename = "story")]
    Story,
    #[serde(rename = "character")]
    Character,
    #[serde(rename = "setting")]
    Setting,
    #[serde(rename = "research")]
    Research,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingDocument {
    #[serde(rename = "file")]
    pub file: FileDocument,
    #[serde(rename = "kind")]
    pub kind: WritingKind,
    #[serde(rename = "section")]
    pub section: WritingSection,
    #[serde(rename = "sidecarRevision")]
    pub sidecar_revision: Decimal,
    #[serde(rename = "font")]
    pub font: WritingFont,
    #[serde(rename = "focusMode")]
    pub focus_mode: bool,
    #[serde(rename = "fullWidth")]
    pub full_width: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingSectionEntry {
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "kind")]
    pub kind: WritingKind,
    #[serde(rename = "section")]
    pub section: WritingSection,
    #[serde(rename = "fileId", skip_serializing_if = "Option::is_none")]
    pub file_id: Option<Id>,
    #[serde(rename = "modifiedAt")]
    pub modified_at: String,
    #[serde(rename = "byteCount")]
    pub byte_count: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingSlide {
    #[serde(rename = "index")]
    pub index: u32,
    #[serde(rename = "startLine")]
    pub start_line: u32,
    #[serde(rename = "endLine")]
    pub end_line: u32,
    #[serde(rename = "heading")]
    pub heading: String,
    #[serde(rename = "notes")]
    pub notes: String,
    #[serde(rename = "imagePaths")]
    pub image_paths: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenplayElement {
    #[serde(rename = "elementId")]
    pub element_id: Id,
    #[serde(rename = "kind")]
    pub kind: ScreenplayElementKind,
    #[serde(rename = "startLine")]
    pub start_line: u32,
    #[serde(rename = "endLine")]
    pub end_line: u32,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "sceneId", skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenplayScene {
    #[serde(rename = "sceneId")]
    pub scene_id: Id,
    #[serde(rename = "ordinal")]
    pub ordinal: u32,
    #[serde(rename = "heading")]
    pub heading: String,
    #[serde(rename = "line")]
    pub line: u32,
    #[serde(rename = "referenceImage", skip_serializing_if = "Option::is_none")]
    pub reference_image: Option<String>,
    #[serde(rename = "caption", skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(rename = "promptIdea", skip_serializing_if = "Option::is_none")]
    pub prompt_idea: Option<String>,
    #[serde(rename = "linkedBeatIds")]
    pub linked_beat_ids: Vec<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariant {
    #[serde(rename = "variantId")]
    pub variant_id: Id,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "author")]
    pub author: FileAuthor,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingLineVariants {
    #[serde(rename = "lineId")]
    pub line_id: Id,
    #[serde(rename = "line")]
    pub line: u32,
    #[serde(rename = "alternates")]
    pub alternates: Vec<WritingVariant>,
    #[serde(rename = "selectedIndex")]
    pub selected_index: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeatCard {
    #[serde(rename = "beatId")]
    pub beat_id: Id,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "body")]
    pub body: String,
    #[serde(rename = "category")]
    pub category: BeatCategory,
    #[serde(rename = "order")]
    pub order: u32,
    #[serde(rename = "sceneId", skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingSectionListParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "section")]
    pub section: WritingSection,
    #[serde(rename = "cursor", skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Id>,
    #[serde(rename = "limit")]
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingSectionListResult {
    #[serde(rename = "entries")]
    pub entries: Vec<WritingSectionEntry>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Id>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingDocumentCreateParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "section")]
    pub section: WritingSection,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "relativeFolder", skip_serializing_if = "Option::is_none")]
    pub relative_folder: Option<String>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingDocumentCreateResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
    #[serde(rename = "pane")]
    pub pane: Pane,
    #[serde(rename = "viewport")]
    pub viewport: FileReadResult,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingDocumentOpenParams {
    #[serde(rename = "workspaceId")]
    pub workspace_id: Id,
    #[serde(rename = "path")]
    pub path: String,
    #[serde(rename = "kind")]
    pub kind: WritingKind,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingDocumentOpenResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
    #[serde(rename = "pane")]
    pub pane: Pane,
    #[serde(rename = "viewport")]
    pub viewport: FileReadResult,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingStructureParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingStructureResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
    #[serde(rename = "slides")]
    pub slides: Vec<WritingSlide>,
    #[serde(rename = "scenes")]
    pub scenes: Vec<ScreenplayScene>,
    #[serde(rename = "elements")]
    pub elements: Vec<ScreenplayElement>,
    #[serde(rename = "variants")]
    pub variants: Vec<WritingLineVariants>,
    #[serde(rename = "beats")]
    pub beats: Vec<BeatCard>,
    #[serde(rename = "wordCount")]
    pub word_count: u32,
    #[serde(rename = "truncated")]
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingFormatParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "range")]
    pub range: TextRange,
    #[serde(rename = "kind")]
    pub kind: WritingFormatKind,
    #[serde(rename = "value", skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "image", skip_serializing_if = "Option::is_none")]
    pub image: Option<AttachmentRef>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingFormatResult {
    #[serde(rename = "file")]
    pub file: FileDocument,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingAssetReadParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "asset")]
    pub asset: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingAssetReadResult {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "image")]
    pub image: AttachmentRef,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingPreferencesParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "font", skip_serializing_if = "Option::is_none")]
    pub font: Option<WritingFont>,
    #[serde(rename = "focusMode", skip_serializing_if = "Option::is_none")]
    pub focus_mode: Option<bool>,
    #[serde(rename = "fullWidth", skip_serializing_if = "Option::is_none")]
    pub full_width: Option<bool>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingPreferencesResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantRequestParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "lineId")]
    pub line_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "agentPaneId")]
    pub agent_pane_id: Id,
    #[serde(rename = "instruction", skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantRequestResult {
    #[serde(rename = "requestId")]
    pub request_id: Id,
    #[serde(rename = "delivered")]
    pub delivered: bool,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantAddParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "lineId")]
    pub line_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "text")]
    pub text: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantSelectParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "lineId")]
    pub line_id: Id,
    #[serde(rename = "variantId")]
    pub variant_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantRemoveParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "lineId")]
    pub line_id: Id,
    #[serde(rename = "variantId")]
    pub variant_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingVariantResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
    #[serde(rename = "variants")]
    pub variants: WritingLineVariants,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingBeatListParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingBeatListResult {
    #[serde(rename = "beats")]
    pub beats: Vec<BeatCard>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingBeatPutParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "beatId", skip_serializing_if = "Option::is_none")]
    pub beat_id: Option<Id>,
    #[serde(rename = "title")]
    pub title: String,
    #[serde(rename = "body")]
    pub body: String,
    #[serde(rename = "category")]
    pub category: BeatCategory,
    #[serde(rename = "order")]
    pub order: u32,
    #[serde(rename = "sceneId", skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<Id>,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingBeatDeleteParams {
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "baseRevision")]
    pub base_revision: Decimal,
    #[serde(rename = "beatId")]
    pub beat_id: Id,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingBeatResult {
    #[serde(rename = "document")]
    pub document: WritingDocument,
    #[serde(rename = "beat", skip_serializing_if = "Option::is_none")]
    pub beat: Option<BeatCard>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WritingChangedEvent {
    #[serde(rename = "eventId")]
    pub event_id: Decimal,
    #[serde(rename = "at")]
    pub at: String,
    #[serde(rename = "fileId")]
    pub file_id: Id,
    #[serde(rename = "paneId")]
    pub pane_id: Id,
    #[serde(rename = "revision")]
    pub revision: Decimal,
    #[serde(rename = "sidecarRevision")]
    pub sidecar_revision: Decimal,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HarnessSource {
    #[serde(rename = "built_in")]
    BuiltIn,
    #[serde(rename = "custom")]
    Custom,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HarnessSetupState {
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "not_checked")]
    NotChecked,
    #[serde(rename = "not_installed")]
    NotInstalled,
    #[serde(rename = "sign_in")]
    SignIn,
    #[serde(rename = "check_setup")]
    CheckSetup,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HarnessSetupReason {
    #[serde(rename = "never_started")]
    NeverStarted,
    #[serde(rename = "executable_missing")]
    ExecutableMissing,
    #[serde(rename = "registry_package_unverified")]
    RegistryPackageUnverified,
    #[serde(rename = "authentication_required")]
    AuthenticationRequired,
    #[serde(rename = "session_failed")]
    SessionFailed,
    #[serde(rename = "launch_failed")]
    LaunchFailed,
    #[serde(rename = "environment_unavailable")]
    EnvironmentUnavailable,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HarnessEnvironmentIssue {
    #[serde(rename = "missing_shell")]
    MissingShell,
    #[serde(rename = "unsupported_shell")]
    UnsupportedShell,
    #[serde(rename = "unavailable_shell")]
    UnavailableShell,
    #[serde(rename = "probe_failed")]
    ProbeFailed,
    #[serde(rename = "probe_timed_out")]
    ProbeTimedOut,
    #[serde(rename = "output_too_large")]
    OutputTooLarge,
    #[serde(rename = "invalid_output")]
    InvalidOutput,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarnessEnvironmentVariable {
    #[serde(rename = "key")]
    pub key: String,
    #[serde(rename = "value")]
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExperimentHookEvent {
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure,
    #[serde(rename = "Stop")]
    Stop,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentHookParams {
    #[serde(rename = "event")]
    pub event: ExperimentHookEvent,
    #[serde(rename = "capability")]
    pub capability: String,
    #[serde(rename = "idempotencyKey")]
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PaneExperimentHookResult {
    #[serde(rename = "recorded")]
    pub recorded: bool,
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
    #[serde(rename = "workspace.layout.set")]
    WorkspaceLayoutSet(WorkspaceLayoutSetParams),
    #[serde(rename = "workspace.close")]
    WorkspaceClose(WorkspaceCloseParams),
    #[serde(rename = "workspace.focus")]
    WorkspaceFocus(WorkspaceFocusParams),
    #[serde(rename = "pane.create")]
    PaneCreate(PaneCreateParams),
    #[serde(rename = "pane.role.set")]
    PaneRoleSet(PaneRoleSetParams),
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
    #[serde(rename = "pane.experiment.set")]
    PaneExperimentSet(PaneExperimentSetParams),
    #[serde(rename = "pane.experiment.delete")]
    PaneExperimentDelete(PaneExperimentDeleteParams),
    #[serde(rename = "pane.experiment.export")]
    PaneExperimentExport(PaneExperimentExportParams),
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
    #[serde(rename = "agent.list")]
    AgentList(AgentListParams),
    #[serde(rename = "agent.message")]
    AgentMessage(AgentMessageParams),
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
    #[serde(rename = "pane.harness.switch")]
    PaneHarnessSwitch(PaneHarnessSwitchParams),
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
    #[serde(rename = "harness.custom.list")]
    HarnessCustomList(HarnessCustomListParams),
    #[serde(rename = "harness.custom.create")]
    HarnessCustomCreate(HarnessCustomCreateParams),
    #[serde(rename = "harness.custom.update")]
    HarnessCustomUpdate(HarnessCustomUpdateParams),
    #[serde(rename = "harness.custom.remove")]
    HarnessCustomRemove(HarnessCustomRemoveParams),
    #[serde(rename = "model.list")]
    ModelList(ModelListParams),
    #[serde(rename = "pane.queue.resume")]
    PaneQueueResume(PaneQueueResumeParams),
    #[serde(rename = "task.create")]
    TaskCreate(TaskCreateParams),
    #[serde(rename = "task.next")]
    TaskNext(TaskNextParams),
    #[serde(rename = "task.show")]
    TaskShow(TaskShowParams),
    #[serde(rename = "task.list")]
    TaskList(TaskListParams),
    #[serde(rename = "task.comments")]
    TaskComments(TaskCommentsParams),
    #[serde(rename = "task.update")]
    TaskUpdate(TaskUpdateParams),
    #[serde(rename = "task.comment")]
    TaskComment(TaskCommentParams),
    #[serde(rename = "task.subitem.add")]
    TaskSubitemAdd(TaskSubitemAddParams),
    #[serde(rename = "task.subitem.done")]
    TaskSubitemDone(TaskSubitemDoneParams),
    #[serde(rename = "task.block")]
    TaskBlock(TaskBlockParams),
    #[serde(rename = "task.release")]
    TaskRelease(TaskReleaseParams),
    #[serde(rename = "task.request_review")]
    TaskRequestReview(TaskRequestReviewParams),
    #[serde(rename = "task.complete")]
    TaskComplete(TaskCompleteParams),
    #[serde(rename = "task.dispatch")]
    TaskDispatch(TaskDispatchParams),
    #[serde(rename = "task.unblock")]
    TaskUnblock(TaskUnblockParams),
    #[serde(rename = "task.move")]
    TaskMove(TaskMoveParams),
    #[serde(rename = "task.worktree.clean")]
    TaskWorktreeClean(TaskWorktreeCleanParams),
    #[serde(rename = "loop.set")]
    LoopSet(LoopSetParams),
    #[serde(rename = "usage.list")]
    UsageList(UsageListParams),
    #[serde(rename = "usage.refresh")]
    UsageRefresh(UsageRefreshParams),
    #[serde(rename = "file.open")]
    FileOpen(FileOpenParams),
    #[serde(rename = "file.read")]
    FileRead(FileReadParams),
    #[serde(rename = "file.write")]
    FileWrite(FileWriteParams),
    #[serde(rename = "file.watch")]
    FileWatch(FileWatchParams),
    #[serde(rename = "file.history")]
    FileHistory(FileHistoryParams),
    #[serde(rename = "file.revert")]
    FileRevert(FileRevertParams),
    #[serde(rename = "file.search")]
    FileSearch(FileSearchParams),
    #[serde(rename = "file.symbols")]
    FileSymbols(FileSymbolsParams),
    #[serde(rename = "file.definition")]
    FileDefinition(FilePositionParams),
    #[serde(rename = "file.references")]
    FileReferences(FilePositionParams),
    #[serde(rename = "file.diagnostics")]
    FileDiagnostics(FileDiagnosticsParams),
    #[serde(rename = "file.diff")]
    FileDiff(FileDiffParams),
    #[serde(rename = "file.format")]
    FileFormat(FileFormatParams),
    #[serde(rename = "editor.review.accept")]
    EditorReviewAccept(EditorReviewParams),
    #[serde(rename = "editor.review.reject")]
    EditorReviewReject(EditorReviewParams),
    #[serde(rename = "editor.review.comment")]
    EditorReviewComment(EditorReviewCommentParams),
    #[serde(rename = "lsp.status")]
    LspStatus(LspStatusParams),
    #[serde(rename = "servers.local.list")]
    ServersLocalList(ServersLocalListParams),
    #[serde(rename = "web.open")]
    WebOpen(WebOpenParams),
    #[serde(rename = "web.navigate")]
    WebNavigate(WebNavigateParams),
    #[serde(rename = "web.snapshot")]
    WebSnapshot(WebSnapshotParams),
    #[serde(rename = "web.click")]
    WebClick(WebClickParams),
    #[serde(rename = "web.type")]
    WebType(WebTypeParams),
    #[serde(rename = "web.press")]
    WebPress(WebPressParams),
    #[serde(rename = "web.hover")]
    WebHover(WebHoverParams),
    #[serde(rename = "web.select")]
    WebSelect(WebSelectParams),
    #[serde(rename = "web.scroll")]
    WebScroll(WebScrollParams),
    #[serde(rename = "web.wait")]
    WebWait(WebWaitParams),
    #[serde(rename = "web.evaluate")]
    WebEvaluate(WebEvaluateParams),
    #[serde(rename = "web.screenshot")]
    WebScreenshot(WebScreenshotParams),
    #[serde(rename = "web.console")]
    WebConsole(WebConsoleParams),
    #[serde(rename = "web.network")]
    WebNetwork(WebNetworkParams),
    #[serde(rename = "web.resize")]
    WebResize(WebResizeParams),
    #[serde(rename = "web.close")]
    WebClose(WebCloseParams),
    #[serde(rename = "web.command")]
    WebCommand(WebCommandParams),
    #[serde(rename = "web.host.navigated")]
    WebHostNavigated(WebHostNavigatedParams),
    #[serde(rename = "web.host.console_counts")]
    WebHostConsoleCounts(WebHostConsoleCountsParams),
    #[serde(rename = "writing.section.list")]
    WritingSectionList(WritingSectionListParams),
    #[serde(rename = "writing.document.create")]
    WritingDocumentCreate(WritingDocumentCreateParams),
    #[serde(rename = "writing.document.open")]
    WritingDocumentOpen(WritingDocumentOpenParams),
    #[serde(rename = "writing.structure")]
    WritingStructure(WritingStructureParams),
    #[serde(rename = "writing.format")]
    WritingFormat(WritingFormatParams),
    #[serde(rename = "writing.asset.read")]
    WritingAssetRead(WritingAssetReadParams),
    #[serde(rename = "writing.preferences.set")]
    WritingPreferencesSet(WritingPreferencesParams),
    #[serde(rename = "writing.variant.request")]
    WritingVariantRequest(WritingVariantRequestParams),
    #[serde(rename = "writing.variant.add")]
    WritingVariantAdd(WritingVariantAddParams),
    #[serde(rename = "writing.variant.select")]
    WritingVariantSelect(WritingVariantSelectParams),
    #[serde(rename = "writing.variant.remove")]
    WritingVariantRemove(WritingVariantRemoveParams),
    #[serde(rename = "writing.beat.list")]
    WritingBeatList(WritingBeatListParams),
    #[serde(rename = "writing.beat.put")]
    WritingBeatPut(WritingBeatPutParams),
    #[serde(rename = "writing.beat.delete")]
    WritingBeatDelete(WritingBeatDeleteParams),
    #[serde(rename = "pane.experiment.hook")]
    PaneExperimentHook(PaneExperimentHookParams),
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
        "workspace.layout.set",
        "WorkspaceLayoutSetParams",
        "WorkspaceLayoutSetResult",
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
    (
        "pane.role.set",
        "PaneRoleSetParams",
        "PaneRoleSetResult",
        true,
    ),
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
        "pane.experiment.set",
        "PaneExperimentSetParams",
        "PaneExperimentSetResult",
        true,
    ),
    (
        "pane.experiment.delete",
        "PaneExperimentDeleteParams",
        "PaneExperimentDeleteResult",
        true,
    ),
    (
        "pane.experiment.export",
        "PaneExperimentExportParams",
        "PaneExperimentExportResult",
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
    ("agent.list", "AgentListParams", "AgentListResult", false),
    (
        "agent.message",
        "AgentMessageParams",
        "AgentMessageResult",
        true,
    ),
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
    (
        "pane.harness.switch",
        "PaneHarnessSwitchParams",
        "PaneHarnessSwitchResult",
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
    (
        "harness.custom.list",
        "HarnessCustomListParams",
        "HarnessCustomListResult",
        false,
    ),
    (
        "harness.custom.create",
        "HarnessCustomCreateParams",
        "HarnessCustomMutationResult",
        true,
    ),
    (
        "harness.custom.update",
        "HarnessCustomUpdateParams",
        "HarnessCustomMutationResult",
        true,
    ),
    (
        "harness.custom.remove",
        "HarnessCustomRemoveParams",
        "HarnessCustomRemoveResult",
        true,
    ),
    ("model.list", "ModelListParams", "ModelListResult", false),
    (
        "pane.queue.resume",
        "PaneQueueResumeParams",
        "PaneQueueResumeResult",
        true,
    ),
    ("task.create", "TaskCreateParams", "TaskCreateResult", true),
    ("task.next", "TaskNextParams", "TaskNextResult", true),
    ("task.show", "TaskShowParams", "TaskShowResult", false),
    ("task.list", "TaskListParams", "TaskListResult", false),
    (
        "task.comments",
        "TaskCommentsParams",
        "TaskCommentsResult",
        false,
    ),
    ("task.update", "TaskUpdateParams", "TaskUpdateResult", true),
    (
        "task.comment",
        "TaskCommentParams",
        "TaskCommentResult",
        true,
    ),
    (
        "task.subitem.add",
        "TaskSubitemAddParams",
        "TaskSubitemAddResult",
        true,
    ),
    (
        "task.subitem.done",
        "TaskSubitemDoneParams",
        "TaskSubitemDoneResult",
        true,
    ),
    ("task.block", "TaskBlockParams", "TaskBlockResult", true),
    (
        "task.release",
        "TaskReleaseParams",
        "TaskReleaseResult",
        true,
    ),
    (
        "task.request_review",
        "TaskRequestReviewParams",
        "TaskRequestReviewResult",
        true,
    ),
    (
        "task.complete",
        "TaskCompleteParams",
        "TaskCompleteResult",
        true,
    ),
    (
        "task.dispatch",
        "TaskDispatchParams",
        "TaskDispatchResult",
        true,
    ),
    (
        "task.unblock",
        "TaskUnblockParams",
        "TaskUnblockResult",
        true,
    ),
    ("task.move", "TaskMoveParams", "TaskMoveResult", true),
    (
        "task.worktree.clean",
        "TaskWorktreeCleanParams",
        "TaskWorktreeCleanResult",
        true,
    ),
    ("loop.set", "LoopSetParams", "LoopSetResult", true),
    ("usage.list", "UsageListParams", "UsageListResult", false),
    (
        "usage.refresh",
        "UsageRefreshParams",
        "UsageRefreshResult",
        true,
    ),
    ("file.open", "FileOpenParams", "FileOpenResult", true),
    ("file.read", "FileReadParams", "FileReadResult", false),
    ("file.write", "FileWriteParams", "FileWriteResult", true),
    ("file.watch", "FileWatchParams", "FileWatchResult", true),
    (
        "file.history",
        "FileHistoryParams",
        "FileHistoryResult",
        false,
    ),
    ("file.revert", "FileRevertParams", "FileRevertResult", true),
    ("file.search", "FileSearchParams", "FileSearchResult", true),
    (
        "file.symbols",
        "FileSymbolsParams",
        "FileSymbolsResult",
        false,
    ),
    (
        "file.definition",
        "FilePositionParams",
        "FileDefinitionResult",
        false,
    ),
    (
        "file.references",
        "FilePositionParams",
        "FileReferencesResult",
        false,
    ),
    (
        "file.diagnostics",
        "FileDiagnosticsParams",
        "FileDiagnosticsResult",
        false,
    ),
    ("file.diff", "FileDiffParams", "FileDiffResult", false),
    ("file.format", "FileFormatParams", "FileFormatResult", true),
    (
        "editor.review.accept",
        "EditorReviewParams",
        "EditorReviewResult",
        true,
    ),
    (
        "editor.review.reject",
        "EditorReviewParams",
        "EditorReviewResult",
        true,
    ),
    (
        "editor.review.comment",
        "EditorReviewCommentParams",
        "EditorReviewCommentResult",
        true,
    ),
    ("lsp.status", "LspStatusParams", "LspStatusResult", false),
    (
        "servers.local.list",
        "ServersLocalListParams",
        "ServersLocalListResult",
        false,
    ),
    ("web.open", "WebOpenParams", "WebOpenResult", true),
    (
        "web.navigate",
        "WebNavigateParams",
        "WebNavigateResult",
        true,
    ),
    (
        "web.snapshot",
        "WebSnapshotParams",
        "WebSnapshotResult",
        false,
    ),
    ("web.click", "WebClickParams", "WebActionResult", true),
    ("web.type", "WebTypeParams", "WebActionResult", true),
    ("web.press", "WebPressParams", "WebActionResult", true),
    ("web.hover", "WebHoverParams", "WebActionResult", true),
    ("web.select", "WebSelectParams", "WebActionResult", true),
    ("web.scroll", "WebScrollParams", "WebActionResult", true),
    ("web.wait", "WebWaitParams", "WebWaitResult", false),
    (
        "web.evaluate",
        "WebEvaluateParams",
        "WebEvaluateResult",
        true,
    ),
    (
        "web.screenshot",
        "WebScreenshotParams",
        "WebScreenshotResult",
        false,
    ),
    ("web.console", "WebConsoleParams", "WebConsoleResult", false),
    ("web.network", "WebNetworkParams", "WebNetworkResult", false),
    ("web.resize", "WebResizeParams", "WebResizeResult", true),
    ("web.close", "WebCloseParams", "WebCloseResult", true),
    ("web.command", "WebCommandParams", "WebCommandResult", false),
    (
        "web.host.navigated",
        "WebHostNavigatedParams",
        "WebHostNavigatedResult",
        true,
    ),
    (
        "web.host.console_counts",
        "WebHostConsoleCountsParams",
        "WebHostConsoleCountsResult",
        true,
    ),
    (
        "writing.section.list",
        "WritingSectionListParams",
        "WritingSectionListResult",
        false,
    ),
    (
        "writing.document.create",
        "WritingDocumentCreateParams",
        "WritingDocumentCreateResult",
        true,
    ),
    (
        "writing.document.open",
        "WritingDocumentOpenParams",
        "WritingDocumentOpenResult",
        true,
    ),
    (
        "writing.structure",
        "WritingStructureParams",
        "WritingStructureResult",
        false,
    ),
    (
        "writing.format",
        "WritingFormatParams",
        "WritingFormatResult",
        true,
    ),
    (
        "writing.asset.read",
        "WritingAssetReadParams",
        "WritingAssetReadResult",
        false,
    ),
    (
        "writing.preferences.set",
        "WritingPreferencesParams",
        "WritingPreferencesResult",
        true,
    ),
    (
        "writing.variant.request",
        "WritingVariantRequestParams",
        "WritingVariantRequestResult",
        true,
    ),
    (
        "writing.variant.add",
        "WritingVariantAddParams",
        "WritingVariantResult",
        true,
    ),
    (
        "writing.variant.select",
        "WritingVariantSelectParams",
        "WritingVariantResult",
        true,
    ),
    (
        "writing.variant.remove",
        "WritingVariantRemoveParams",
        "WritingVariantResult",
        true,
    ),
    (
        "writing.beat.list",
        "WritingBeatListParams",
        "WritingBeatListResult",
        false,
    ),
    (
        "writing.beat.put",
        "WritingBeatPutParams",
        "WritingBeatResult",
        true,
    ),
    (
        "writing.beat.delete",
        "WritingBeatDeleteParams",
        "WritingBeatResult",
        true,
    ),
    (
        "pane.experiment.hook",
        "PaneExperimentHookParams",
        "PaneExperimentHookResult",
        true,
    ),
];
pub const PANE_ROUTE_METHODS: &[(&str, &str)] = &[
    ("pane.role.set", "paneId"),
    ("pane.close", "paneId"),
    ("pane.focus", "paneId"),
    ("pane.rename", "paneId"),
    ("pane.resize", "paneId"),
    ("pane.input", "paneId"),
    ("pane.stream.subscribe", "paneId"),
    ("pane.stream.unsubscribe", "paneId"),
    ("pane.record.set", "paneId"),
    ("pane.experiment.set", "paneId"),
    ("pane.experiment.delete", "paneId"),
    ("pane.prompt", "paneId"),
    ("agent.message", "targetPaneId"),
    ("pane.queue.list", "paneId"),
    ("pane.queue.remove", "paneId"),
    ("pane.cancel", "paneId"),
    ("pane.permission.answer", "paneId"),
    ("pane.elicitation.answer", "paneId"),
    ("pane.config.set", "paneId"),
    ("pane.harness.switch", "paneId"),
    ("pane.yolo", "paneId"),
    ("pane.auth.start", "paneId"),
    ("pane.transcript", "paneId"),
    ("pane.draft", "paneId"),
    ("model.list", "paneId"),
    ("pane.queue.resume", "paneId"),
    ("task.next", "paneId"),
    ("task.comment", "paneId"),
    ("task.subitem.add", "paneId"),
    ("task.subitem.done", "paneId"),
    ("task.block", "paneId"),
    ("task.release", "paneId"),
    ("task.request_review", "paneId"),
    ("task.complete", "paneId"),
    ("loop.set", "paneId"),
    ("web.navigate", "paneId"),
    ("web.snapshot", "paneId"),
    ("web.click", "paneId"),
    ("web.type", "paneId"),
    ("web.press", "paneId"),
    ("web.hover", "paneId"),
    ("web.select", "paneId"),
    ("web.scroll", "paneId"),
    ("web.wait", "paneId"),
    ("web.evaluate", "paneId"),
    ("web.screenshot", "paneId"),
    ("web.console", "paneId"),
    ("web.network", "paneId"),
    ("web.resize", "paneId"),
    ("web.close", "paneId"),
    ("web.command", "paneId"),
    ("web.host.navigated", "paneId"),
    ("web.host.console_counts", "paneId"),
    ("writing.variant.request", "agentPaneId"),
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EventName {
    #[serde(rename = "auth.challenge")]
    AuthChallenge,
    #[serde(rename = "state.snapshot_part")]
    StateSnapshotPart,
    #[serde(rename = "workspace.created")]
    WorkspaceCreated,
    #[serde(rename = "workspace.renamed")]
    WorkspaceRenamed,
    #[serde(rename = "workspace.layout_updated")]
    WorkspaceLayoutUpdated,
    #[serde(rename = "workspace.closed")]
    WorkspaceClosed,
    #[serde(rename = "pane.created")]
    PaneCreated,
    #[serde(rename = "pane.renamed")]
    PaneRenamed,
    #[serde(rename = "pane.closed")]
    PaneClosed,
    #[serde(rename = "pane.state_changed")]
    PaneStateChanged,
    #[serde(rename = "stream.replay_start")]
    StreamReplayStart,
    #[serde(rename = "stream.replay_done")]
    StreamReplayDone,
    #[serde(rename = "stream.gap")]
    StreamGap,
    #[serde(rename = "device.pair_requested")]
    DevicePairRequested,
    #[serde(rename = "device.paired")]
    DevicePaired,
    #[serde(rename = "device.revoked")]
    DeviceRevoked,
    #[serde(rename = "pair.result")]
    PairResult,
    #[serde(rename = "engine.updating")]
    EngineUpdating,
    #[serde(rename = "engine.restarted")]
    EngineRestarted,
    #[serde(rename = "ptyd.reconnected")]
    PtydReconnected,
    #[serde(rename = "ptyd.lost")]
    PtydLost,
    #[serde(rename = "pane.updated")]
    PaneUpdated,
    #[serde(rename = "settings.changed")]
    SettingsChanged,
    #[serde(rename = "pane.session_started")]
    PaneSessionStarted,
    #[serde(rename = "pane.turn_ended")]
    PaneTurnEnded,
    #[serde(rename = "pane.queued")]
    PaneQueued,
    #[serde(rename = "pane.permission_requested")]
    PanePermissionRequested,
    #[serde(rename = "pane.permission_answered")]
    PanePermissionAnswered,
    #[serde(rename = "pane.elicitation_requested")]
    PaneElicitationRequested,
    #[serde(rename = "pane.elicitation_answered")]
    PaneElicitationAnswered,
    #[serde(rename = "pane.config_changing")]
    PaneConfigChanging,
    #[serde(rename = "pane.config_changed")]
    PaneConfigChanged,
    #[serde(rename = "pane.harness_switch_started")]
    PaneHarnessSwitchStarted,
    #[serde(rename = "pane.harness_switch_done")]
    PaneHarnessSwitchDone,
    #[serde(rename = "pane.harness_switch_failed")]
    PaneHarnessSwitchFailed,
    #[serde(rename = "pane.config_failed")]
    PaneConfigFailed,
    #[serde(rename = "pane.session_lost")]
    PaneSessionLost,
    #[serde(rename = "pane.needs_login")]
    PaneNeedsLogin,
    #[serde(rename = "pane.auth_finished")]
    PaneAuthFinished,
    #[serde(rename = "pane.update")]
    PaneUpdate,
    #[serde(rename = "attention.changed")]
    AttentionChanged,
    #[serde(rename = "pane.queue.paused")]
    PaneQueuePaused,
    #[serde(rename = "pane.queue.resumed")]
    PaneQueueResumed,
    #[serde(rename = "task.created")]
    TaskCreated,
    #[serde(rename = "task.moved")]
    TaskMoved,
    #[serde(rename = "task.claimed")]
    TaskClaimed,
    #[serde(rename = "task.released")]
    TaskReleased,
    #[serde(rename = "task.commented")]
    TaskCommented,
    #[serde(rename = "task.subitem_changed")]
    TaskSubitemChanged,
    #[serde(rename = "task.blocked")]
    TaskBlocked,
    #[serde(rename = "task.unblocked")]
    TaskUnblocked,
    #[serde(rename = "task.phrase_changed")]
    TaskPhraseChanged,
    #[serde(rename = "task.updated")]
    TaskUpdated,
    #[serde(rename = "task.attention")]
    TaskAttention,
    #[serde(rename = "loop.tick")]
    LoopTick,
    #[serde(rename = "loop.stopped")]
    LoopStopped,
    #[serde(rename = "usage.changed")]
    UsageChanged,
    #[serde(rename = "file.changed_on_disk")]
    FileChangedOnDisk,
    #[serde(rename = "file.saved")]
    FileSaved,
    #[serde(rename = "file.conflict")]
    FileConflict,
    #[serde(rename = "lsp.diagnostics")]
    LspDiagnostics,
    #[serde(rename = "editor.agent_cursor")]
    EditorAgentCursor,
    #[serde(rename = "editor.review_requested")]
    EditorReviewRequested,
    #[serde(rename = "servers.local.changed")]
    ServersLocalChanged,
    #[serde(rename = "web.navigated")]
    WebNavigated,
    #[serde(rename = "web.console_counts")]
    WebConsoleCounts,
    #[serde(rename = "web.agent_driving")]
    WebAgentDriving,
    #[serde(rename = "writing.changed")]
    WritingChanged,
}
impl EventName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthChallenge => "auth.challenge",
            Self::StateSnapshotPart => "state.snapshot_part",
            Self::WorkspaceCreated => "workspace.created",
            Self::WorkspaceRenamed => "workspace.renamed",
            Self::WorkspaceLayoutUpdated => "workspace.layout_updated",
            Self::WorkspaceClosed => "workspace.closed",
            Self::PaneCreated => "pane.created",
            Self::PaneRenamed => "pane.renamed",
            Self::PaneClosed => "pane.closed",
            Self::PaneStateChanged => "pane.state_changed",
            Self::StreamReplayStart => "stream.replay_start",
            Self::StreamReplayDone => "stream.replay_done",
            Self::StreamGap => "stream.gap",
            Self::DevicePairRequested => "device.pair_requested",
            Self::DevicePaired => "device.paired",
            Self::DeviceRevoked => "device.revoked",
            Self::PairResult => "pair.result",
            Self::EngineUpdating => "engine.updating",
            Self::EngineRestarted => "engine.restarted",
            Self::PtydReconnected => "ptyd.reconnected",
            Self::PtydLost => "ptyd.lost",
            Self::PaneUpdated => "pane.updated",
            Self::SettingsChanged => "settings.changed",
            Self::PaneSessionStarted => "pane.session_started",
            Self::PaneTurnEnded => "pane.turn_ended",
            Self::PaneQueued => "pane.queued",
            Self::PanePermissionRequested => "pane.permission_requested",
            Self::PanePermissionAnswered => "pane.permission_answered",
            Self::PaneElicitationRequested => "pane.elicitation_requested",
            Self::PaneElicitationAnswered => "pane.elicitation_answered",
            Self::PaneConfigChanging => "pane.config_changing",
            Self::PaneConfigChanged => "pane.config_changed",
            Self::PaneHarnessSwitchStarted => "pane.harness_switch_started",
            Self::PaneHarnessSwitchDone => "pane.harness_switch_done",
            Self::PaneHarnessSwitchFailed => "pane.harness_switch_failed",
            Self::PaneConfigFailed => "pane.config_failed",
            Self::PaneSessionLost => "pane.session_lost",
            Self::PaneNeedsLogin => "pane.needs_login",
            Self::PaneAuthFinished => "pane.auth_finished",
            Self::PaneUpdate => "pane.update",
            Self::AttentionChanged => "attention.changed",
            Self::PaneQueuePaused => "pane.queue.paused",
            Self::PaneQueueResumed => "pane.queue.resumed",
            Self::TaskCreated => "task.created",
            Self::TaskMoved => "task.moved",
            Self::TaskClaimed => "task.claimed",
            Self::TaskReleased => "task.released",
            Self::TaskCommented => "task.commented",
            Self::TaskSubitemChanged => "task.subitem_changed",
            Self::TaskBlocked => "task.blocked",
            Self::TaskUnblocked => "task.unblocked",
            Self::TaskPhraseChanged => "task.phrase_changed",
            Self::TaskUpdated => "task.updated",
            Self::TaskAttention => "task.attention",
            Self::LoopTick => "loop.tick",
            Self::LoopStopped => "loop.stopped",
            Self::UsageChanged => "usage.changed",
            Self::FileChangedOnDisk => "file.changed_on_disk",
            Self::FileSaved => "file.saved",
            Self::FileConflict => "file.conflict",
            Self::LspDiagnostics => "lsp.diagnostics",
            Self::EditorAgentCursor => "editor.agent_cursor",
            Self::EditorReviewRequested => "editor.review_requested",
            Self::ServersLocalChanged => "servers.local.changed",
            Self::WebNavigated => "web.navigated",
            Self::WebConsoleCounts => "web.console_counts",
            Self::WebAgentDriving => "web.agent_driving",
            Self::WritingChanged => "writing.changed",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventDescriptor {
    pub name: EventName,
    pub params_type: &'static str,
    pub durable: bool,
}
pub const EVENTS: &[EventDescriptor] = &[
    EventDescriptor {
        name: EventName::AuthChallenge,
        params_type: "AuthChallengeEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::StateSnapshotPart,
        params_type: "StateSnapshotPartEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::WorkspaceCreated,
        params_type: "WorkspaceCreatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::WorkspaceRenamed,
        params_type: "WorkspaceRenamedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::WorkspaceLayoutUpdated,
        params_type: "WorkspaceLayoutUpdatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::WorkspaceClosed,
        params_type: "WorkspaceClosedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneCreated,
        params_type: "PaneCreatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneRenamed,
        params_type: "PaneRenamedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneClosed,
        params_type: "PaneClosedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneStateChanged,
        params_type: "PaneStateChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::StreamReplayStart,
        params_type: "StreamReplayStartEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::StreamReplayDone,
        params_type: "StreamReplayDoneEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::StreamGap,
        params_type: "StreamGapEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::DevicePairRequested,
        params_type: "DevicePairRequestedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::DevicePaired,
        params_type: "DevicePairedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::DeviceRevoked,
        params_type: "DeviceRevokedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PairResult,
        params_type: "PairResultEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::EngineUpdating,
        params_type: "EngineUpdatingEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::EngineRestarted,
        params_type: "EngineRestartedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PtydReconnected,
        params_type: "PtydReconnectedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PtydLost,
        params_type: "PtydLostEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneUpdated,
        params_type: "PaneUpdatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::SettingsChanged,
        params_type: "SettingsChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneSessionStarted,
        params_type: "PaneSessionStartedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneTurnEnded,
        params_type: "PaneTurnEndedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneQueued,
        params_type: "PaneQueuedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PanePermissionRequested,
        params_type: "PanePermissionRequestedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PanePermissionAnswered,
        params_type: "PanePermissionAnsweredEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneElicitationRequested,
        params_type: "PaneElicitationRequestedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneElicitationAnswered,
        params_type: "PaneElicitationAnsweredEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneConfigChanging,
        params_type: "PaneConfigChangingEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneConfigChanged,
        params_type: "PaneConfigChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneHarnessSwitchStarted,
        params_type: "PaneHarnessSwitchStartedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneHarnessSwitchDone,
        params_type: "PaneHarnessSwitchDoneEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneHarnessSwitchFailed,
        params_type: "PaneHarnessSwitchFailedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneConfigFailed,
        params_type: "PaneConfigFailedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneSessionLost,
        params_type: "PaneSessionLostEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneNeedsLogin,
        params_type: "PaneNeedsLoginEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneAuthFinished,
        params_type: "PaneAuthFinishedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneUpdate,
        params_type: "PaneUpdateEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::AttentionChanged,
        params_type: "AttentionChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneQueuePaused,
        params_type: "PaneQueueStateEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::PaneQueueResumed,
        params_type: "PaneQueueStateEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskCreated,
        params_type: "TaskCreatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskMoved,
        params_type: "TaskMovedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskClaimed,
        params_type: "TaskClaimedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskReleased,
        params_type: "TaskReleasedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskCommented,
        params_type: "TaskCommentedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskSubitemChanged,
        params_type: "TaskSubitemChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskBlocked,
        params_type: "TaskBlockedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskUnblocked,
        params_type: "TaskUnblockedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskPhraseChanged,
        params_type: "TaskPhraseChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskUpdated,
        params_type: "TaskUpdatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::TaskAttention,
        params_type: "TaskAttentionEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::LoopTick,
        params_type: "LoopTickEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::LoopStopped,
        params_type: "LoopStoppedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::UsageChanged,
        params_type: "UsageChangedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::FileChangedOnDisk,
        params_type: "FileChangedOnDiskEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::FileSaved,
        params_type: "FileSavedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::FileConflict,
        params_type: "FileConflictEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::LspDiagnostics,
        params_type: "LspDiagnosticsEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::EditorAgentCursor,
        params_type: "EditorAgentCursorEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::EditorReviewRequested,
        params_type: "EditorReviewRequestedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::ServersLocalChanged,
        params_type: "ServersLocalChangedEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::WebNavigated,
        params_type: "WebNavigatedEvent",
        durable: true,
    },
    EventDescriptor {
        name: EventName::WebConsoleCounts,
        params_type: "WebConsoleCountsEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::WebAgentDriving,
        params_type: "WebAgentDrivingEvent",
        durable: false,
    },
    EventDescriptor {
        name: EventName::WritingChanged,
        params_type: "WritingChangedEvent",
        durable: true,
    },
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
        ErrorKind::EngineIdentityChanged => -32020,
        ErrorKind::StaleAttempt => -32021,
        ErrorKind::DependencyCycle => -32022,
        ErrorKind::DependencyOpen => -32023,
        ErrorKind::RateLimited => -32024,
        ErrorKind::StaleReference => -32025,
        ErrorKind::HumanTookOver => -32026,
        ErrorKind::WebHostUnavailable => -32027,
        ErrorKind::NavigationRefused => -32028,
        ErrorKind::WritingKindRefused => -32029,
        ErrorKind::WritingLimitExceeded => -32030,
        ErrorKind::WritingSidecarConflict => -32031,
        ErrorKind::WritingLineNotFound => -32032,
        ErrorKind::WritingSceneNotFound => -32033,
        ErrorKind::WritingSectionRefused => -32034,
        ErrorKind::HarnessSwitchFailed => -32035,
    }
}
