// Generated from contracts/protocol.json. Do not edit.

use crate::{ConfigOption, HighlightKind, PaneState, UsageProvider};

pub const REQUEST_TIMEOUT_MS: u64 = 12000;
pub const ENGINE_DEADLINE_MARGIN_MS: u64 = 5000;
pub const GREETING_TIMEOUT_MS: u64 = 5000;
pub const RECONNECT_DELAY_MS: u64 = 1000;
pub const RECONNECT_CEILING_MS: u64 = 30000;
pub const DRAFT_BEAT_MS: u64 = 8000;
pub const AGENT_PARTIAL_FLUSH_MS: u64 = 16;
pub const DIAGNOSTICS_MS: u64 = 30000;
pub const COPIED_FEEDBACK_MS: u64 = 1500;
pub const SERVICE_COMMAND_MS: u64 = 60000;
pub const WEB_READ_SETTLE_POLL_MS: u64 = 80;
pub const WEB_READ_DEADLINE_SHARE: f64 = 0.8;
pub const TERMINAL_INPUT_CHUNK_BYTES: usize = 16384;
pub const TERMINAL_INPUT_QUEUE_BYTES: usize = 1048576;
pub const TERMINAL_SCROLLBACK_LINES: usize = 10000;
pub const CAPS_WAITING_MESSAGES: usize = 1000;
pub const CAPS_FILE_LINKS: usize = 128;
pub const CAPS_WEB_LINKS: usize = 128;
pub const CAPS_MODEL_PAGES: usize = 32;
pub const CAPS_TICKER_HISTORY: usize = 3;
pub const CAPS_DIAGNOSTICS_REPORT_BYTES: usize = 4194304;
pub const CAPS_SERVICE_REPORT_BYTES: usize = 65536;
pub const PAGE_SIZE_QUEUE: u32 = 64;
pub const PAGE_SIZE_HARNESSES: u32 = 64;
pub const PAGE_SIZE_MODELS: u32 = 64;
pub const PAGE_SIZE_TRANSCRIPT: u32 = 100;
pub const PAGE_SIZE_TASKS: u32 = 100;
pub const PAGE_SIZE_WRITING_SECTION: u32 = 100;
pub const HARNESS_SWITCH_TITLE: &str = "Switch agent";
pub const HARNESS_SWITCH_HANDOVER: &str = "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.";
pub const HARNESSES_LOADING: &str = "Loading agents…";
pub const HARNESSES_NONE: &str = "No other agents";
pub const HARNESSES_RESCAN: &str = "Check agents again";
pub const CANCEL: &str = "Cancel";
pub const TASK_MOVE_REASON: &str = "Moved on the workspace board";
pub const EDITOR_NO_FILE: &str = "The engine has no file for this editor tab.";
pub const EDITOR_FILE_NOT_TEXT: &str = "Binary file, can't be shown.";
pub const EDITOR_FILE_TOO_LARGE: &str = "This file is too large to open in the editor.";
pub const EDITOR_SAVE_MISSING: &str =
    "Couldn't save: this file was deleted from the disk. Save again to create it.";
pub const EDITOR_SAVE_DENIED: &str =
    "Couldn't save: permission denied. Fix the permissions, then save again.";
pub const EDITOR_SAVE_FULL: &str =
    "Couldn't save: the disk is full. Free some space, then save again.";
pub const EDITOR_SAVE_OTHER: &str =
    "Couldn't save this file. The engine keeps trying; save again to retry now.";
pub const EDITOR_SAVE_AGAIN: &str = "Save again";
pub const DIAGNOSTICS_TITLE: &str = "Diagnostics";
pub const DIAGNOSTICS_SUMMARY: &str = "Copies a report of this Soyeht and its recent logs, with paths, names, addresses and tokens removed, to paste into a support message.";
pub const DIAGNOSTICS_ACTION: &str = "Copy diagnostics";
pub const DIAGNOSTICS_WORKING: &str = "Collecting diagnostics";
pub const DIAGNOSTICS_COPIED: &str = "Diagnostics copied. Paste them into your message.";
pub const DIAGNOSTICS_FAILED: &str = "Soyeht could not collect the diagnostics.";
pub const DIAGNOSTICS_UNAVAILABLE: &str = "This copy of Soyeht has no diagnostics program.";
pub const COPY_CODE: &str = "Copy code";
pub const COPIED_CODE: &str = "Copied";
pub const CRASH_NOTICE_TITLE: &str = "Soyeht closed unexpectedly";
pub const CRASH_NOTICE_SUMMARY: &str =
    "A crash report was saved on this computer. Nothing was sent.";
pub const CRASH_NOTICE_COPY: &str = "Copy report";
pub const CRASH_NOTICE_DISMISS: &str = "Dismiss";
pub const LOGIN_START_QUESTION: &str = "Keep Soyeht running in the background?";
pub const LOGIN_START_SUMMARY: &str = "Soyeht's engine keeps your terminals and agents running when this window is closed, and starts when you log in.";
pub const LOGIN_START_LATER: &str = "You can turn it on later from Preferences.";
pub const LOGIN_START_TURN_ON: &str = "Turn on";
pub const LOGIN_START_NOT_NOW: &str = "Not now";
pub const TRY_AGAIN: &str = "Try again";
pub const OK: &str = "OK";
pub const APP_MOVE_QUESTION: &str = "Move Soyeht to Applications?";
pub const APP_MOVE_SUMMARY: &str = "Soyeht's engine needs the app in your Applications folder so macOS remembers the permissions you give it. Soyeht copies itself there and opens again. If it runs from a disk image, you can eject the image afterwards.";
pub const APP_MOVE_ACTION: &str = "Move to Applications";
pub const APP_MOVE_MENU: &str = "Move to Applications…";
pub const APP_MOVE_EXPLANATION: &str = "The engine is off. It needs Soyeht in your Applications folder so macOS remembers its permissions.";
pub const APP_MOVE_REPLACE_QUESTION: &str = "Replace the app in Applications?";
pub const APP_MOVE_REPLACE_SUMMARY: &str =
    "Soyeht is already in Applications. The old copy goes to the Trash.";
pub const APP_MOVE_REPLACE: &str = "Replace";
pub const APP_MOVE_FAILED_TITLE: &str = "Soyeht could not move itself";
pub const APP_MOVE_FAILED_LATER: &str = "The engine stays off until Soyeht is in Applications. You can drag it there yourself, or choose Move to Applications again.";
pub const APP_MOVE_RELAUNCH_FAILED_TITLE: &str = "Soyeht is now in Applications";
pub const APP_MOVE_RELAUNCH_FAILED: &str =
    "Soyeht was moved to Applications but could not open there. Open it from Applications.";
pub const APP_MOVE_NOT_THIS_APP: &str =
    "This copy is not the app Soyeht expected, so nothing was changed.";
pub const APP_MOVE_ANOTHER_APP: &str =
    "A different app with the same name is in Applications. Soyeht did not touch it.";
pub const APP_MOVE_NO_WRITABLE_FOLDER: &str =
    "Neither the Applications folder nor your own Applications folder can be written to.";
pub const APP_MOVE_CHANGED: &str =
    "Applications changed while Soyeht was moving. Nothing was replaced.";
pub const APP_MOVE_NOT_FINISHED: &str =
    "The copy did not finish. The app that was there is unchanged.";
pub const APP_MOVE_NOT_SIGNED: &str = "The copy is not signed like this app, so it was not installed. The app that was there is unchanged.";
pub const APP_MOVE_ALREADY_THERE: &str =
    "This is the copy that is in Applications. Open it from there.";
pub const APP_MOVE_RUNNING: &str =
    "Soyeht is running from Applications. Quit it there, then move this copy.";
pub const APP_MOVE_COPY_LOCKED: &str = "A copy of Soyeht is already in Applications and cannot be replaced from this account. Remove it, or open that copy.";
pub const APP_MOVE_OLD_IN_TRASH: &str = "The old copy of Soyeht is in the Trash and could not be put back. Drag it from the Trash to Applications to restore it.";
pub const PUSH_NEEDS_YOU_TITLE: &str = "An agent needs you";
pub const PUSH_NEEDS_YOU_BODY: &str = "Open Soyeht to see what it asks.";
pub const CLIENT_NAME_MAC: &str = "mac";
pub const CLIENT_NAME_MAC_DEV: &str = "mac-dev";
pub const CLIENT_NAME_IPHONE: &str = "iphone";
pub const CLIENT_NAME_IPHONE_DEV: &str = "iphone-dev";
pub const CLIENT_NAME_DESKTOP: &str = "desktop";
pub const CLIENT_NAME_DESKTOP_DEV: &str = "desktop-dev";
pub const CLIENT_NAME_ANDROID: &str = "android";
pub const CLIENT_NAME_ANDROID_DEV: &str = "android-dev";
pub const USAGE_BAND_WARNING_FROM: f64 = 0.5;
pub const USAGE_BAND_DANGER_ABOVE: f64 = 0.8;
pub const CONTEXT_BAND_WARNING_ABOVE: f64 = 0.75;
pub const CONTEXT_BAND_DANGER_ABOVE: f64 = 0.9;
pub const USAGE_RING_LAST_KNOWN_ARC_OPACITY: f64 = 0.45;
pub const USAGE_RING_UNAVAILABLE_OPACITY: f64 = 0.46;
pub const USAGE_WINDOW_WEEKLY_KEYWORD: &str = "week";
pub const CRASH_REPORTS_FOLDER: &str = "crashes";
pub const CRASH_REPORTS_INCOMING_PREFIX: &str = "incoming-";
pub const EDITOR_PREFETCH_LINES: u32 = 160;
pub const EDITOR_PAGE_LEAD_DIVISOR: u32 = 4;
pub const EDITOR_TAB_WIDTH: u32 = 4;
pub const EDITOR_TAB_HISTORY: u32 = 64;
pub const EDITOR_SAVE_ATTEMPTS: u32 = 30;
pub const EDITOR_SAVE_RETRY_MS: u32 = 100;
pub const EDITOR_INDENT_UNIT: &str = "    ";
pub const EDITOR_INDENT_OPENERS: &str = "{[(";
/// Image types that claude-agent-acp and codex-acp forward as model input and that both
/// providers accept; any other image is linked.
pub const INLINE_IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
pub const MODEL_CATEGORY: &str = "model";
pub const EFFORT_CATEGORY: &str = "thought_level";
pub const PERMISSION_MODE_CATEGORY: &str = "mode";

/// The palette role a pane state's dot is colored from; each client resolves the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateDot {
    Success,
    Accent,
    Warning,
    Muted,
    Secondary,
}

impl PaneState {
    /// The wire spelling of the state (`waiting_for_you`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::WaitingForYou => "waiting_for_you",
            Self::Cancelling => "cancelling",
            Self::NoSignal => "no_signal",
            Self::Recovering => "recovering",
            Self::Dead => "dead",
            Self::Unknown => "unknown",
        }
    }

    /// Whether an agent in this state is in the middle of a turn: it can be stopped, what
    /// is typed is queued, and the header counts the time.
    pub fn is_turn_running(&self) -> bool {
        matches!(
            self,
            Self::Working | Self::WaitingForYou | Self::Cancelling | Self::NoSignal
        )
    }

    /// The role of the state's dot.
    pub fn dot(&self) -> StateDot {
        match self {
            Self::Starting => StateDot::Secondary,
            Self::Working => StateDot::Success,
            Self::Idle => StateDot::Success,
            Self::WaitingForYou => StateDot::Accent,
            Self::Cancelling => StateDot::Secondary,
            Self::NoSignal => StateDot::Warning,
            Self::Recovering => StateDot::Secondary,
            Self::Dead => StateDot::Muted,
            Self::Unknown => StateDot::Muted,
        }
    }
}

/// The character an editor types after `opening` to close the pair, when it closes one
/// (brackets and quotes).
pub fn editor_closing(opening: &str) -> Option<&'static str> {
    match opening {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "\"" => Some("\""),
        "'" => Some("'"),
        _ => None,
    }
}

/// Whether `typed` is a character that closes a pair (a closing bracket or a quote), so
/// typing it over the same character already there steps over it.
pub fn editor_closes_pair(typed: &str) -> bool {
    matches!(typed, ")" | "]" | "}" | "\"" | "'")
}

/// The palette role a syntax highlight is colored from; each client resolves the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightRole {
    Accent,
    SyntaxType,
    Warning,
    Muted,
    Success,
    Ink,
}

impl HighlightKind {
    /// The role of the highlight's color.
    pub fn role(&self) -> HighlightRole {
        match self {
            Self::Keyword => HighlightRole::Accent,
            Self::String => HighlightRole::Success,
            Self::Comment => HighlightRole::Muted,
            Self::Number => HighlightRole::Warning,
            Self::Type => HighlightRole::SyntaxType,
            Self::Function => HighlightRole::Accent,
            Self::Property => HighlightRole::Ink,
            Self::Operator => HighlightRole::Accent,
            Self::Punctuation => HighlightRole::Ink,
            Self::Constant => HighlightRole::Warning,
            Self::Variable => HighlightRole::Ink,
        }
    }
}

impl UsageProvider {
    /// The name the clients show for the provider.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
            Self::Opencode => "OpenCode",
        }
    }

    /// The provider whose meter reads a harness: the first rule of `usageHarnesses` whose text the
    /// harness id holds, whatever the case.
    pub fn for_harness(harness_id: &str) -> Option<Self> {
        let id = harness_id.to_lowercase();
        [
            ("claude", Self::Claude),
            ("codex", Self::Codex),
            ("cursor", Self::Cursor),
            ("opencode", Self::Opencode),
        ]
        .into_iter()
        .find(|(fragment, _)| id.contains(fragment))
        .map(|(_, provider)| provider)
    }
}

impl ConfigOption {
    // A selector has a role when its category (its id when it has none) names it.

    /// Whether this option is the agent's model selector.
    pub fn is_model(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == MODEL_CATEGORY
    }

    /// Whether this option is the agent's effort selector.
    pub fn is_effort(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == EFFORT_CATEGORY || self.id == "effort"
    }

    /// Whether this option is the agent's permission selector.
    pub fn is_permission_mode(&self) -> bool {
        self.category.as_deref().unwrap_or(&self.id) == PERMISSION_MODE_CATEGORY
            || self.id == "mode"
    }
}
