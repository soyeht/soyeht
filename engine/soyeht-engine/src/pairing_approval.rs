//! How the person approves a pairing on each system (ADR 0164).
//!
//! The rule is chosen by the platform and never by a failed attestation: macOS keeps the signed
//! app's alone, Linux asks the person to type the code the new device shows, and Windows refuses
//! until its app proves itself. In the `PersonsCode` mode the engine makes a six-digit code when a
//! device proves its pairing request, keeps it in memory only, and approves the request only
//! when the person types it at the computer.
//!
//! The code lives only here: it is never a field of a pending request (which the store keeps),
//! of a receipt, of a log line or of an event, and it has no `Debug`, `Serialize` or `Clone`.

use crate::errors::{self, Result};
use soyeht_protocol::{
    approval_code::{self, WRONG_CODES_ALLOWED},
    limits,
};
use std::collections::BTreeMap;
use zeroize::Zeroize;

/// Pairing windows one local connection keeps open; a new one closes its oldest.
const WINDOWS_PER_CONNECTION: usize = 4;

/// Whether the engine's peer rules of ADR 0164 (slice 3) are in place. Until they are, a program
/// of the same user could be the new device, so no system runs `PersonsCode` by itself: a test
/// selects it through `Hooks::pairing_approval`. This may become true only in the change that
/// brings slice 3 of the ADR (the listener, address, `whois` and node rules); a test below stops
/// the build when it is flipped alone.
const PEER_RULES_ENFORCED: bool = false;

/// Who may approve a pairing on this system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approval {
    /// The signed Soyeht app, which the engine attests (macOS).
    SignedApp,
    /// The person at the computer, with the code the new device shows (Linux).
    PersonsCode,
    /// Nobody, until the app can prove itself (Windows).
    Refused,
}

impl Approval {
    /// The rule of the system this engine runs on.
    pub fn for_this_system() -> Self {
        Self::for_system(std::env::consts::OS, PEER_RULES_ENFORCED)
    }

    fn for_system(os: &str, peer_rules_enforced: bool) -> Self {
        match os {
            "macos" => Self::SignedApp,
            "linux" if peer_rules_enforced => Self::PersonsCode,
            _ => Self::Refused,
        }
    }

    /// Whether a local connection that is the human at the computer may start a window, decide a
    /// request and revoke another device. `signed_ui` is whether the engine attested the
    /// connection as the app.
    pub(crate) fn admits_local_human(self, signed_ui: bool) -> bool {
        match self {
            Self::PersonsCode => true,
            Self::SignedApp | Self::Refused => signed_ui,
        }
    }

    /// Whether this engine asks for the code (and so offers the `pairing-code` capability).
    pub fn asks_for_code(self) -> bool {
        self == Self::PersonsCode
    }
}

/// The six digits of one request. No `Debug`, `Serialize` or `Clone`: nothing can print, store or
/// copy it by accident, and its memory is cleared when it is dropped.
pub(crate) struct ApprovalCode([u8; approval_code::DIGITS]);

impl ApprovalCode {
    fn generate() -> Result<Self> {
        Self::from_random(|| {
            let mut bytes = [0u8; 4];
            getrandom::fill(&mut bytes).map_err(|_| errors::Error::RandomUnavailable)?;
            Ok(u32::from_le_bytes(bytes))
        })
    }

    /// Uniform digits from `next` by rejection: a draw in the last, partial run of the range is
    /// thrown away, so no digit is likelier than another.
    fn from_random(mut next: impl FnMut() -> Result<u32>) -> Result<Self> {
        const SPAN: u32 = 10u32.pow(approval_code::DIGITS as u32);
        const LIMIT: u32 = u32::MAX / SPAN * SPAN;
        loop {
            let mut draw = next()?;
            if draw >= LIMIT {
                continue;
            }
            draw %= SPAN;
            let mut digits = [b'0'; approval_code::DIGITS];
            for slot in digits.iter_mut().rev() {
                *slot = b'0' + (draw % 10) as u8;
                draw /= 10;
            }
            return Ok(Self(digits));
        }
    }

    /// Whether `typed` is this code, in time that does not depend on where they differ.
    fn matches(&self, typed: &[u8; approval_code::DIGITS]) -> bool {
        self.0
            .iter()
            .zip(typed)
            .fold(0u8, |difference, (own, other)| difference | (own ^ other))
            == 0
    }

    /// The digits, for the one answer that carries them: the new device's.
    fn reveal(&self) -> String {
        self.0.iter().map(|digit| char::from(*digit)).collect()
    }
}

impl Drop for ApprovalCode {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

struct Entry {
    code: ApprovalCode,
    wrong: u8,
    deadline: i64,
}

/// What the person's typing did to a request's code.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The code was right and is spent.
    Right,
    /// Nothing was typed.
    Missing,
    /// What was typed is not six ASCII digits, so it can never be right. It costs no try: counting
    /// it adds no security and three typos would deny the person.
    Malformed,
    /// The code is wrong; `left` tries remain.
    Wrong { left: u8 },
    /// The last try was wrong: the code is gone and the request is to be denied.
    Exhausted,
    /// The engine holds no live code for the request (it restarted, or the code expired).
    NoCode,
}

/// The live codes by request id, in memory only.
#[derive(Default)]
pub(crate) struct Codes(BTreeMap<String, Entry>);

impl Codes {
    /// The code of `request`, made now when it has none that is live, as the digits to send to the
    /// device that proved it.
    pub(crate) fn issue(&mut self, request: &str, deadline: i64, now: i64) -> Result<String> {
        self.0.retain(|_, entry| entry.deadline > now);
        if let Some(entry) = self.0.get(request) {
            return Ok(entry.code.reveal());
        }
        let code = ApprovalCode::generate()?;
        let digits = code.reveal();
        self.0.insert(
            request.to_owned(),
            Entry {
                code,
                wrong: 0,
                deadline,
            },
        );
        Ok(digits)
    }

    pub(crate) fn check(&mut self, request: &str, typed: Option<&str>, now: i64) -> Verdict {
        if self
            .0
            .get(request)
            .is_none_or(|entry| entry.deadline <= now)
        {
            self.0.remove(request);
            return Verdict::NoCode;
        }
        let Some(typed) = typed else {
            return Verdict::Missing;
        };
        let Some(entry) = self.0.get_mut(request) else {
            return Verdict::NoCode;
        };
        let Some(mut digits) = approval_code::read(typed) else {
            return Verdict::Malformed;
        };
        let right = entry.code.matches(&digits);
        digits.zeroize();
        if right {
            self.0.remove(request);
            return Verdict::Right;
        }
        entry.wrong += 1;
        if entry.wrong >= WRONG_CODES_ALLOWED {
            self.0.remove(request);
            return Verdict::Exhausted;
        }
        Verdict::Wrong {
            left: WRONG_CODES_ALLOWED - entry.wrong,
        }
    }

    pub(crate) fn forget(&mut self, request: &str) {
        self.0.remove(request);
    }
}

struct Bound {
    connection: String,
    order: u64,
}

/// The pairing windows, each bound to the local connection that opened it (in memory only). Only
/// that connection decides the requests that come in on its window and sees them listed; an
/// engine restart or the end of the connection leaves the window with no one to decide for it.
#[derive(Default)]
pub(crate) struct Windows {
    bound: BTreeMap<String, Bound>,
    opened: u64,
}

impl Windows {
    /// Binds `window` to `connection`, closing the connection's oldest window when it would hold
    /// more than `WINDOWS_PER_CONNECTION`.
    pub(crate) fn bind(&mut self, window: &str, connection: &str) {
        self.opened += 1;
        self.bound.insert(
            window.to_owned(),
            Bound {
                connection: connection.to_owned(),
                order: self.opened,
            },
        );
        let mut own: Vec<(u64, String)> = self
            .bound
            .iter()
            .filter(|(_, bound)| bound.connection == connection)
            .map(|(window, bound)| (bound.order, window.clone()))
            .collect();
        own.sort();
        let surplus = own.len().saturating_sub(WINDOWS_PER_CONNECTION);
        for (_, window) in own.into_iter().take(surplus) {
            self.bound.remove(&window);
        }
    }

    pub(crate) fn is_bound(&self, window: &str) -> bool {
        self.bound.contains_key(window)
    }

    pub(crate) fn is_bound_to(&self, window: &str, connection: &str) -> bool {
        self.bound
            .get(window)
            .is_some_and(|bound| bound.connection == connection)
    }

    pub(crate) fn release(&mut self, connection: &str) {
        self.bound.retain(|_, bound| bound.connection != connection);
    }
}

/// What pairing keeps in memory between calls: the address a link names, and for the person's code
/// mode the codes and the windows' connections.
pub(crate) struct Pairing {
    pub(crate) address: String,
    pub(crate) windows: Windows,
    pub(crate) codes: Codes,
}

impl Pairing {
    pub(crate) fn at(address: String) -> Self {
        Self {
            address,
            windows: Windows::default(),
            codes: Codes::default(),
        }
    }
}

/// The name a new device gives itself is shown to the person, so it holds no control character
/// and none of the characters that reorder, hide or break the text around it (the bidirectional
/// controls, the line and paragraph separators, the zero-width characters and the tag
/// characters), and is no longer than the contract allows.
pub(crate) fn check_device_name(name: &str) -> Result<()> {
    let disguising = |c: char| {
        matches!(
            c,
            '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{200B}'..='\u{200D}'
                | '\u{2060}'
                | '\u{FEFF}'
                | '\u{E0000}'..='\u{E007F}'
        )
    };
    if name.len() > limits::DEVICE_NAME_BYTES
        || name.chars().any(|c| c.is_control() || disguising(c))
    {
        return Err(errors::invalid("The device name is not allowed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compiles only for a type that does not implement the trait: the two impls would be
    /// ambiguous for one that does.
    macro_rules! assert_not_impl {
        ($type:ty, $($trait:path),+) => {
            $(const _: fn() = || {
                trait AmbiguousIfImpl<A> { fn check() {} }
                impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
                struct Invalid;
                impl<T: ?Sized + $trait> AmbiguousIfImpl<Invalid> for T {}
                let _ = <$type as AmbiguousIfImpl<_>>::check;
            };)+
        };
    }
    assert_not_impl!(ApprovalCode, std::fmt::Debug, Clone, serde::Serialize);

    fn code(text: &str) -> ApprovalCode {
        let mut digits = [0u8; approval_code::DIGITS];
        digits.copy_from_slice(text.as_bytes());
        ApprovalCode(digits)
    }

    fn codes_with(request: &str, text: &str, deadline: i64) -> Codes {
        let mut codes = Codes::default();
        codes.0.insert(
            request.into(),
            Entry {
                code: code(text),
                wrong: 0,
                deadline,
            },
        );
        codes
    }

    #[test]
    fn the_rule_follows_the_platform_and_waits_for_the_peer_rules_on_linux() {
        assert_eq!(Approval::for_system("macos", false), Approval::SignedApp);
        assert_eq!(Approval::for_system("macos", true), Approval::SignedApp);
        assert_eq!(Approval::for_system("windows", false), Approval::Refused);
        assert_eq!(Approval::for_system("windows", true), Approval::Refused);
        assert_eq!(Approval::for_system("linux", false), Approval::Refused);
        assert_eq!(Approval::for_system("linux", true), Approval::PersonsCode);
        // Until slice 3 lands, no system selects the person's code by itself.
        assert!(!Approval::for_this_system().asks_for_code());
        #[cfg(target_os = "macos")]
        assert_eq!(Approval::for_this_system(), Approval::SignedApp);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(Approval::for_this_system(), Approval::Refused);
    }

    #[test]
    fn only_the_person_s_code_lets_an_unattested_local_human_manage_devices() {
        assert!(Approval::PersonsCode.admits_local_human(false));
        assert!(!Approval::SignedApp.admits_local_human(false));
        assert!(Approval::SignedApp.admits_local_human(true));
        assert!(!Approval::Refused.admits_local_human(false));
    }

    #[test]
    fn a_code_is_six_uniform_digits_drawn_by_rejection() {
        let span = 1_000_000u32;
        let limit = u32::MAX / span * span;
        let mut draws = vec![u32::MAX, limit, limit - 1].into_iter();
        let drawn = ApprovalCode::from_random(|| Ok(draws.next().unwrap())).unwrap();
        // The two draws in the partial run were thrown away; the first below it is used.
        assert_eq!(drawn.reveal(), format!("{:06}", (limit - 1) % span));
        let mut draws = vec![0u32].into_iter();
        let zero = ApprovalCode::from_random(|| Ok(draws.next().unwrap())).unwrap();
        assert_eq!(zero.reveal(), "000000");
        let real = ApprovalCode::generate().unwrap().reveal();
        assert_eq!(real.len(), approval_code::DIGITS);
        assert!(real.bytes().all(|byte| byte.is_ascii_digit()));
    }

    #[test]
    fn a_dropped_code_is_cleared() {
        let mut held = code("482913");
        held.0.zeroize();
        assert_eq!(held.0, [0; approval_code::DIGITS]);
    }

    #[test]
    fn the_right_code_is_single_use() {
        let mut codes = codes_with("r", "482913", 100);
        assert_eq!(codes.check("r", Some("482 913"), 10), Verdict::Right);
        assert_eq!(codes.check("r", Some("482913"), 10), Verdict::NoCode);
    }

    #[test]
    fn three_wrong_codes_end_the_request_s_code_and_a_right_one_after_them_is_too_late() {
        let mut codes = codes_with("r", "482913", 100);
        assert_eq!(
            codes.check("r", Some("000000"), 10),
            Verdict::Wrong { left: 2 }
        );
        // A text that is not six digits can never be right and costs no try.
        for malformed in ["48291", "4829133", "48291a", "", "482-913"] {
            assert_eq!(
                codes.check("r", Some(malformed), 10),
                Verdict::Malformed,
                "{malformed:?}"
            );
        }
        assert_eq!(
            codes.check("r", Some("000001"), 10),
            Verdict::Wrong { left: 1 }
        );
        assert_eq!(codes.check("r", Some("482914"), 10), Verdict::Exhausted);
        assert_eq!(codes.check("r", Some("482913"), 10), Verdict::NoCode);
    }

    #[test]
    fn a_missing_code_uses_no_try_and_a_late_or_unknown_one_finds_nothing() {
        let mut codes = codes_with("r", "482913", 100);
        for _ in 0..5 {
            assert_eq!(codes.check("r", None, 10), Verdict::Missing);
        }
        assert_eq!(codes.check("r", Some("482913"), 100), Verdict::NoCode);
        assert_eq!(codes.check("other", Some("482913"), 10), Verdict::NoCode);
    }

    #[test]
    fn a_code_is_issued_once_per_request_and_expired_ones_are_swept() {
        let mut codes = Codes::default();
        let first = codes.issue("r", 100, 10).unwrap();
        assert_eq!(codes.issue("r", 100, 11).unwrap(), first);
        codes.issue("old", 20, 10).unwrap();
        codes.issue("new", 100, 30).unwrap();
        assert!(!codes.0.contains_key("old"));
        // A request whose code is gone gets another one.
        codes.forget("r");
        assert_eq!(
            codes.issue("r", 100, 31).unwrap().len(),
            approval_code::DIGITS
        );
    }

    #[test]
    fn a_window_belongs_to_the_connection_that_opened_it_and_the_fifth_closes_the_oldest() {
        let mut windows = Windows::default();
        for index in 0..4 {
            windows.bind(&format!("window-{index}"), "app");
        }
        windows.bind("other-window", "another");
        assert!(windows.is_bound_to("window-0", "app"));
        assert!(!windows.is_bound_to("window-0", "another"));
        windows.bind("window-4", "app");
        assert!(!windows.is_bound("window-0"));
        for kept in ["window-1", "window-2", "window-3", "window-4"] {
            assert!(windows.is_bound_to(kept, "app"), "{kept}");
        }
        assert!(windows.is_bound_to("other-window", "another"));
        windows.release("app");
        assert!(!windows.is_bound("window-4"));
        assert!(windows.is_bound("other-window"));
    }

    #[test]
    fn a_device_name_has_no_control_and_no_disguising_character() {
        assert!(check_device_name("Caio's iPhone").is_ok());
        assert!(check_device_name("ジョンの iPhone").is_ok());
        assert!(check_device_name(&"a".repeat(limits::DEVICE_NAME_BYTES)).is_ok());
        assert!(check_device_name(&"a".repeat(limits::DEVICE_NAME_BYTES + 1)).is_err());
        for bad in [
            "line\nbreak",
            "tab\there",
            "nul\0",
            "esc\u{1b}",
            "next\u{85}line",
            "\u{061C}",
            "\u{200E}",
            "\u{200F}",
            "\u{202A}",
            "evil\u{202E}fdp.exe",
            "\u{202E}",
            "\u{2066}",
            "\u{2069}",
            "line\u{2028}separator",
            "paragraph\u{2029}separator",
            "zero\u{200B}width",
            "joiner\u{200C}",
            "joiner\u{200D}",
            "word\u{2060}joiner",
            "\u{FEFF}bom",
            "tag\u{E0041}",
            "\u{E0000}",
            "\u{E007F}",
        ] {
            assert!(check_device_name(bad).is_err(), "{bad:?}");
        }
        // The neighbours of the ranges are ordinary characters.
        assert!(check_device_name("\u{2065}\u{206A}\u{200A}\u{2061}\u{E0080}").is_ok());
    }

    /// Tripwire: the person's code is for Linux only together with the peer rules of slice 3. If
    /// this stops compiling, the rules are in and this check (and the Linux case of
    /// `the_rule_follows_the_platform_and_waits_for_the_peer_rules_on_linux`) is to be replaced by
    /// the tests that prove them.
    const _: () = assert!(
        !PEER_RULES_ENFORCED,
        "PEER_RULES_ENFORCED may become true only with the slice 3 peer rules of ADR 0164"
    );
}
