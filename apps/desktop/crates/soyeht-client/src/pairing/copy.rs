//! What the pairing view says (ADR 0164). The words a phone and the Mac also use are the shared
//! client policy's (`soyeht_protocol::client_policy`); the rest are the desktop's own and are here
//! and nowhere else, so a view never types one.

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
