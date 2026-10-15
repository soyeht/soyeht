//! The error kinds the supervisor sends in `Response::Error`.
//!
//! This is the supervisor's own vocabulary, separate from the engine's error codes in
//! `contracts/protocol.json` (ADR 0003): some spellings are equal, but the engine maps a
//! supervisor kind to its own code. Both sides name a kind through these constants.

pub const UNAUTHORIZED: &str = "unauthorized";
pub const VERSION_UNSUPPORTED: &str = "version_unsupported";
pub const STALE_ENGINE_GENERATION: &str = "stale_engine_generation";
pub const ENGINE_CONNECTED: &str = "engine_connected";
pub const ALREADY_AUTHENTICATED: &str = "already_authenticated";
pub const OPERATION_FAILED: &str = "operation_failed";
pub const IDEMPOTENCY_CONFLICT: &str = "idempotency_conflict";
pub const SPAWN_FAILED: &str = "spawn_failed";
pub const NOT_FOUND: &str = "not_found";
pub const WRONG_PROCESS_KIND: &str = "wrong_process_kind";
pub const INVALID_PARAMS: &str = "invalid_params";
pub const PROCESS_EXITED: &str = "process_exited";
pub const INPUT_TOO_LARGE: &str = "input_too_large";
pub const ACK_AHEAD: &str = "ack_ahead";
pub const CURSOR_AHEAD: &str = "cursor_ahead";
pub const INVALID_OFFSET: &str = "invalid_offset";
pub const OUTPUT_OFFSET_OVERFLOW: &str = "output_offset_overflow";
