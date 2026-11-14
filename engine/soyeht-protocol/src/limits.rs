// Generated from contracts/protocol.json. Do not edit.
pub const MIN_PROTOCOL_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;
pub const JSON_BYTES: usize = 1048576;
pub const TERMINAL_PAYLOAD_BYTES: usize = 65536;
pub const ATTACHMENT_PAYLOAD_BYTES: usize = 262144;
pub const TERMINAL_HEADER_BYTES: usize = 16;
pub const ATTACHMENT_HEADER_BYTES: usize = 16;
pub const CONNECTION_QUEUE_BYTES: usize = 8388608;
pub const PTY_RETENTION_BYTES: usize = 8388608;
pub const SEGMENT_BYTES: usize = 524288;
pub const PANE_RETENTION_BYTES: usize = 33554432;
pub const TAIL_BYTES: usize = 2097152;
pub const EVENT_COUNT: usize = 10000;
pub const EVENT_DAYS: usize = 7;
pub const DEVICE_CONNECTIONS: usize = 8;
/// The characters of a pairing code, and of an engine's public key, in a pairing link: 32 bytes as URL-safe base64 without padding.
pub const PAIRING_CODE_CHARS: usize = 43;
/// How long a pairing code stays valid, and the longest a device waits for the person to allow access.
pub const PAIRING_WINDOW_SECONDS: usize = 600;
/// The longest name (in UTF-8 bytes) a device gives itself when it pairs. The engine shows it to the person who allows access.
pub const DEVICE_NAME_BYTES: usize = 128;
pub const PROMPT_BYTES: usize = 65536;
pub const PROMPT_QUEUE_COUNT: usize = 64;
pub const AGENT_MESSAGES_PER_WINDOW: usize = 6;
pub const AGENT_MESSAGE_WINDOW_SEC: usize = 60;
pub const AGENT_MESSAGE_BYTES: usize = 60000;
/// The longest name (in characters) the engine puts in the header of a message between agents, and the longest it compares when a message names its recipient. A longer name is cut.
pub const AGENT_NAME_CHARS: usize = 40;
pub const TRANSCRIPT_PAGE_MESSAGES: usize = 200;
pub const TRANSCRIPT_PAGE_BYTES: usize = 524288;
pub const TRANSCRIPT_MESSAGE_BYTES: usize = 262144;
pub const ACP_MESSAGE_BYTES: usize = 1048576;
pub const PERMISSION_TIMEOUT_SEC: usize = 1800;
pub const PERMISSION_PREVIEW_PATH_CHARS: usize = 4096;
pub const PERMISSION_PREVIEW_CHANGES: usize = 8;
pub const PERMISSION_PREVIEW_BYTES: usize = 65536;
pub const PERMISSION_PREVIEW_LINES: usize = 400;
pub const PERMISSION_COMMAND_BYTES: usize = 65536;
pub const CANCEL_TIMEOUT_SEC: usize = 10;
pub const PROBE_TIMEOUT_SEC: usize = 10;
pub const NO_SIGNAL_SEC: usize = 90;
pub const HARNESS_SWITCH_TIMEOUT_SEC: usize = 90;
pub const USAGE_REFRESH_MIN_INTERVAL_SEC: usize = 30;
pub const USAGE_AUTO_REFRESH_INTERVAL_SEC: usize = 300;
pub const USAGE_STALE_AFTER_SEC: usize = 900;
pub const USAGE_MAX_BACKOFF_SEC: usize = 1800;
pub const USAGE_ACTIVITY_DAYS: usize = 7;
pub const EDITOR_VIEWPORT_LINES: usize = 2000;
pub const EDITOR_EDIT_BYTES: usize = 262144;
/// The longest path a client links as a file reference in text (`path:line`). It bounds the clients' detector; the engine does not enforce it, and `file.open` takes the path the schema allows.
pub const EDITOR_PATH_BYTES: usize = 4096;
pub const DIAGNOSTIC_MESSAGE_CHARS: usize = 4096;
pub const EDITOR_SEARCH_RESULTS: usize = 500;
pub const EDITOR_HISTORY_VERSIONS: usize = 200;
pub const EDITOR_HISTORY_BYTES: usize = 52428800;
pub const EDITOR_AUTOSAVE_MS: usize = 500;
pub const PROJECT_TREE_ENTRIES: usize = 5000;
pub const PROJECT_TREE_WATCHES: usize = 256;
pub const AGENT_COMMANDS: usize = 256;
pub const SKILL_BYTES: usize = 262144;
pub const SKILL_ENTRIES: usize = 256;
pub const WEB_PANES_PER_WORKSPACE: usize = 10;
pub const WEB_SNAPSHOT_BYTES: usize = 204800;
pub const WEB_SCREENSHOT_BYTES: usize = 2097152;
pub const WEB_EVALUATE_BYTES: usize = 65536;
pub const WEB_CONSOLE_ENTRIES: usize = 500;
pub const WEB_NETWORK_ENTRIES: usize = 300;
pub const WEB_WAIT_TIMEOUT_MS: usize = 60000;
pub const LOCAL_SERVER_ENTRIES: usize = 50;
pub const WEB_PICKED_ELEMENTS: usize = 20;
pub const WEB_OUTER_HTML_BYTES: usize = 2048;
pub const WEB_FIELD_BYTES: usize = 4096;
pub const WEB_TEXT_BYTES: usize = 16384;
pub const WEB_LABEL_BYTES: usize = 2048;
pub const WEB_TITLE_BYTES: usize = 1024;
pub const WEB_PART_BYTES: usize = 512;
pub const WEB_TOKEN_BYTES: usize = 256;
pub const WEB_EXTENT_MAX: usize = 16384;
pub const WEB_PICK_CROP_WIDTH: usize = 640;
pub const WEB_READ_SCREENS: usize = 12;
pub const WEB_READ_SCREENS_DEFAULT: usize = 4;
pub const WEB_READ_TEXT_BYTES: usize = 65536;
pub const WEB_READ_LINKS: usize = 100;
pub const WEB_READ_BYTES: usize = 524288;
pub const WEB_AGENT_IMAGE_BYTES: usize = 262144;
pub const PROMPT_ATTACHMENTS: usize = 20;
pub const PROMPT_ATTACHMENT_BYTES: usize = 10485760;
pub const PROMPT_ATTACHMENT_TOTAL_BYTES: usize = 20971520;
pub const PROMPT_ATTACHMENT_INLINE_BYTES: usize = 196608;
pub const PROMPT_ATTACHMENT_THUMBNAIL_BYTES: usize = 6144;
pub const WEB_COMMAND_WAITERS: usize = 64;
pub const PORT_SCAN_BYTES: usize = 1048576;
pub const WRITING_SECTION_ENTRIES: usize = 500;
pub const WRITING_SLIDES: usize = 500;
pub const WRITING_SCENES: usize = 1000;
pub const WRITING_ELEMENTS: usize = 10000;
pub const WRITING_BEAT_CARDS: usize = 500;
pub const WRITING_SCENE_LINKS: usize = 1000;
pub const WRITING_VARIANTS_PER_LINE: usize = 12;
pub const WRITING_VARIANT_BYTES: usize = 16384;
pub const WRITING_SUGGESTION_BYTES: usize = 65536;
pub const WRITING_SIDECAR_BYTES: usize = 1048576;
pub const WRITING_TITLE_BYTES: usize = 512;
pub const WRITING_PATH_BYTES: usize = 4096;
pub const WRITING_IMAGE_BYTES: usize = 2097152;
pub const HARNESS_CONTINUATION_BYTES: usize = 15999;
pub const HARNESS_CONTINUATION_MESSAGES: usize = 6;
pub const MODEL_REFUSALS: usize = 256;
pub const MODEL_REFUSAL_REASON_BYTES: usize = 4096;
pub const EXPERIMENT_EXPORT_PANES: usize = 64;
pub const EXPERIMENT_EXPORT_ROWS: usize = 100000;
pub const EXPERIMENT_EXPORT_BYTES: usize = 67108864;
/// The most strokes one `ink.recognize` request carries.
pub const INK_STROKES: usize = 64;
/// The most points one stroke of an `ink.recognize` request carries.
pub const INK_STROKE_POINTS: usize = 2000;
/// The most points all the strokes of one `ink.recognize` request carry together.
pub const INK_TOTAL_POINTS: usize = 8000;
/// The most candidates an `ink.recognize` request may ask for and the engine answers with.
pub const INK_CANDIDATES: usize = 10;
/// How many candidates an `ink.recognize` request without `maxCandidates` gets.
pub const INK_CANDIDATES_DEFAULT: usize = 5;
/// The most language tags one `ink.recognize` request names.
pub const INK_LANGUAGES: usize = 8;
/// The longest language tag (in UTF-8 bytes) of an `ink.recognize` request; the length a BCP 47 tag may have.
pub const INK_LANGUAGE_BYTES: usize = 35;
/// The largest absolute value of a stroke coordinate in an `ink.recognize` request, in the client's own units (the engine scales the writing, so the unit is the client's choice).
pub const INK_COORDINATE: usize = 1000000;
/// The longest a stroke point's time since the first point of the writing may be in an `ink.recognize` request, in milliseconds.
pub const INK_DURATION_MS: usize = 3600000;
/// The longest text (in UTF-8 bytes) of one candidate the engine answers `ink.recognize` with; a longer one is cut at a character.
pub const INK_CANDIDATE_BYTES: usize = 2048;
/// The longest the engine takes to answer one `ink.recognize` request, the wait behind other recognitions included; one that outlives it is answered with `internal_error`.
pub const INK_DEADLINE_SEC: usize = 10;
pub const FRAME_KIND_JSON: u8 = 1;
pub const FRAME_KIND_TERMINAL: u8 = 2;
pub const FRAME_KIND_ATTACHMENT: u8 = 3;
