//! What the pairing view says (ADR 0164). The words a phone and the Mac also use are the shared
//! client policy's (`soyeht_protocol::client_policy`); the rest are the desktop's own and are here
//! and nowhere else, so a view never types one.

use crate::timestamp;
use soyeht_protocol::{EngineReachability, EngineReachabilityReason};

/// The entry that starts a pairing, and the title of its sheet.
pub const PAIR_BUTTON: &str = "Pair a phone";
/// The place's name and what it holds, as the Mac's place of the same name says.
pub const PHONE_TITLE: &str = "iPhone or iPad";
pub const PHONE_SUBTITLE: &str = "Paired devices can continue every workspace.";
pub const DEVICES_TITLE: &str = "Paired devices";
pub const NO_DEVICES: &str = "No paired device";
/// Under the QR code, when the phone can reach this computer.
pub const SCAN_INSTRUCTION: &str = "Open Soyeht on your iPhone or iPad and scan this code. Both devices must be on the same tailnet.";
pub const COPY_LINK: &str = "Copy link";
pub const CLOSE: &str = "Close";
pub const ALLOW: &str = "Allow";
pub const DENY: &str = "Deny";
pub const DISMISS: &str = "Dismiss";
pub const REVOKE: &str = "Revoke";
pub const REVOKE_QUESTION: &str = "Revoke this device's access?";
pub const REVOKE_NOTE: &str = "Connections will close. Access will require pairing again.";
pub const PAIRED: &str = "Paired";
pub const REVOKED: &str = "Revoked";
/// A request that came in while the pairing sheet was closed waits in the list of devices.
pub const WAITING_FOR_APPROVAL: &str = "Waiting for approval";
/// Where the engine will not pair, in front of the engine's own words.
pub const CANNOT_PAIR_YET: &str = "This computer cannot pair a phone yet.";
/// The sheet while it asks the engine for a window.
pub const STARTING: &str = "Making a pairing code\u{2026}";
/// The window ran out.
pub const EXPIRED: &str = "This code expired. Make a new one to pair a phone.";
/// The connection to the engine went while the sheet was open: the window it opened went with it.
pub const INTERRUPTED: &str = "The connection to the engine was lost, so this code no longer works. A new one is made when it is back.";
/// What a request says it is for. The phone shows the same short code beside the six digits.
pub const COMPARE: &str =
    "Allow only if your phone shows this same code. If it shows another code or none, deny.";
pub const NO_COMPARISON: &str =
    "This request has no code to compare, so it cannot be checked. Deny it.";
pub const TYPE_THE_CODE: &str = "Type the code your phone shows.";
pub const CODE_PLACEHOLDER: &str = "Six digits";
pub const CODE_WARNING: &str = "Type a code only if your own phone is showing it now, for a pairing you started. Nobody else will ask for it.";
pub const PHONE_SHOWS_NO_CODE: &str = "If your phone shows no code, update Soyeht on the phone.";
pub const MALFORMED_CODE: &str = "Type the six digits your phone shows.";
pub const REQUEST_GONE: &str =
    "This request is not waiting any more: it was decided, denied or it expired.";
pub const REVOKE_FAILED: &str = "Soyeht could not revoke this device.";
/// The notice of a device added, which stays until the person dismisses it.
pub const NOTICE_TITLE: &str = "A device was added";
pub const NOTICE_NOTE: &str = "If you did not pair it, revoke it.";
pub const NOTICE_FAILED: &str = "Soyeht could not read or dismiss the notice of added devices.";
/// What the question before a revocation says of the panes the device made.
pub const PANES_UNKNOWN: &str = "Soyeht could not list the panes this device opened.";
pub const PANES_NOTE: &str = "Closing a pane ends its process group, not a process that detached itself from it. Revoking a device does not stop what its terminals started.";
pub const REVOKE_AND_KEEP: &str = "Revoke, keep the panes";

/// A device in the notice and in the list: when and from where it was added, in the words of the
/// engine's own record. A device that has no date (it was paired before dating, or on a Mac) says
/// nothing.
pub fn added_line(added_at: Option<&str>, node: Option<&str>) -> Option<String> {
    let when = added_at
        .and_then(timestamp::seconds_since_epoch)
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds as i64, 0))
        .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string());
    match (when, node) {
        (Some(when), Some(node)) => Some(format!("Added {when} from {node}")),
        (Some(when), None) => Some(format!("Added {when}")),
        (None, Some(node)) => Some(format!("Added from {node}")),
        (None, None) => None,
    }
}

/// What the question says of the panes a device opened.
pub fn panes_opened(count: usize) -> String {
    match count {
        1 => "This device opened 1 pane that is still open:".to_owned(),
        count => format!("This device opened {count} panes that are still open:"),
    }
}

/// The button that revokes a device and closes the panes it opened.
pub fn revoke_and_close(count: usize) -> String {
    match count {
        1 => "Revoke and close the pane".to_owned(),
        count => format!("Revoke and close {count} panes"),
    }
}

/// What the person is told when some of the panes they chose to close would not close.
pub fn panes_left_open(count: usize) -> String {
    match count {
        1 => "The device was revoked, but 1 of its panes could not be closed.".to_owned(),
        count => format!("The device was revoked, but {count} of its panes could not be closed."),
    }
}

/// The question a request puts to the person.
pub fn allow_question(device_name: &str) -> String {
    format!("Allow \u{201C}{device_name}\u{201D}?")
}

/// The time a window has left, as `9:42`.
pub fn time_left(seconds: u64) -> String {
    format!("Valid for {}:{:02}", seconds / 60, seconds % 60)
}

/// Why a phone cannot reach this computer, for the person to act on, or `None` when it can.
pub fn reachability_problem(reachability: &EngineReachability) -> Option<String> {
    if reachability.remote {
        return None;
    }
    let detail = match &reachability.reason {
        Some(EngineReachabilityReason::TailnetLookupFailed) => {
            "the network interfaces could not be read"
        }
        Some(EngineReachabilityReason::TailnetListenFailed) => {
            "Soyeht could not listen on the Tailscale address"
        }
        Some(EngineReachabilityReason::TailnetPortInUse) => {
            "another program on this computer is using the port Soyeht listens on; Soyeht connects by itself once the port is free"
        }
        Some(EngineReachabilityReason::LoopbackOnly) => "this computer listens on loopback only",
        Some(EngineReachabilityReason::TailnetAddressNotFound) | None => {
            "Tailscale address not found"
        }
        Some(_) => "the reason is not known to this version of Soyeht",
    };
    Some(format!("Not reachable from your devices: {detail}"))
}
