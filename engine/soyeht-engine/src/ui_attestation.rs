#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::{
        base::{CFGetTypeID, TCFType},
        data::CFData,
        dictionary::{CFDictionaryGetValue, CFDictionaryRef},
        string::{CFString, CFStringGetTypeID, CFStringRef},
    };
    use security_framework::os::macos::code_signing::{
        Flags, GuestAttributes, SecCode, SecRequirement,
    };
    use soyeht_protocol::profile::Profile;
    use std::{
        ffi::c_void,
        mem::{MaybeUninit, size_of},
        os::fd::{AsRawFd, RawFd},
        ptr, slice,
        str::FromStr,
        sync::OnceLock,
    };
    use tokio::net::UnixStream;

    const LOCAL_PEERTOKEN: libc::c_int = 0x006;
    const SIGNING_INFORMATION: u32 = 1 << 1;
    static ENGINE_TEAM: OnceLock<Option<String>> = OnceLock::new();

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct PeerAuditToken {
        words: [u32; 8],
    }

    type SecCodeRef = *mut c_void;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecCodeInfoTeamIdentifier: CFStringRef;
        fn SecCodeCopySigningInformation(
            code: SecCodeRef,
            flags: u32,
            information: *mut CFDictionaryRef,
        ) -> i32;
    }

    fn read_audit_token(descriptor: RawFd) -> Option<PeerAuditToken> {
        let mut token = MaybeUninit::<PeerAuditToken>::uninit();
        let mut length = libc::socklen_t::try_from(size_of::<PeerAuditToken>()).ok()?;
        // SAFETY: `token` points to `length` writable bytes and both live for
        // the call. LOCAL_PEERTOKEN returns one fixed-size audit_token_t.
        let status = unsafe {
            libc::getsockopt(
                descriptor,
                libc::SOL_LOCAL,
                LOCAL_PEERTOKEN,
                token.as_mut_ptr().cast(),
                &mut length,
            )
        };
        if status != 0 || usize::try_from(length).ok()? != size_of::<PeerAuditToken>() {
            return None;
        }
        // SAFETY: getsockopt succeeded and reported the complete structure.
        Some(unsafe { token.assume_init() })
    }

    pub fn peer_audit_token(stream: &UnixStream) -> Option<PeerAuditToken> {
        read_audit_token(stream.as_raw_fd())
    }

    fn closed_team(team: String) -> Option<String> {
        (team.len() == 10
            && team
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
        .then_some(team)
    }

    fn signing_team(code: &SecCode) -> Option<String> {
        let mut information = ptr::null();
        // SAFETY: `code` is a retained SecCode, `information` is an out pointer,
        // and the create-rule dictionary is released before returning.
        let status = unsafe {
            SecCodeCopySigningInformation(
                code.as_concrete_TypeRef().cast(),
                SIGNING_INFORMATION,
                &mut information,
            )
        };
        if status != 0 || information.is_null() {
            return None;
        }
        // SAFETY: a successful copy returns one owned CFDictionary.
        let information = unsafe {
            core_foundation::dictionary::CFDictionary::<*const c_void, *const c_void>::wrap_under_create_rule(information)
        };
        // SAFETY: both objects are live Core Foundation values owned by the
        // Security framework and the retained dictionary, respectively.
        let value = unsafe {
            CFDictionaryGetValue(
                information.as_concrete_TypeRef(),
                kSecCodeInfoTeamIdentifier.cast(),
            )
        };
        if value.is_null() {
            return None;
        }
        // SAFETY: both functions only inspect valid Core Foundation objects.
        if unsafe { CFGetTypeID(value.cast()) } != unsafe { CFStringGetTypeID() } {
            return None;
        }
        // SAFETY: the type check above proves this borrowed value is a CFString.
        let team = unsafe { CFString::wrap_under_get_rule(value.cast()) }.to_string();
        closed_team(team)
    }

    fn engine_team(profile: &Profile) -> Option<&'static str> {
        ENGINE_TEAM
            .get_or_init(|| {
                let code = SecCode::for_self(Flags::NONE).ok()?;
                let requirement = SecRequirement::from_str(&format!(
                    "anchor apple generic and identifier \"{}\"",
                    profile.engine_launchd_label
                ))
                .ok()?;
                code.check_validity(
                    Flags::STRICT_VALIDATE | Flags::NO_NETWORK_ACCESS,
                    &requirement,
                )
                .ok()?;
                signing_team(&code)
            })
            .as_deref()
    }

    pub fn signed_ui(token: PeerAuditToken, profile: &Profile) -> bool {
        let Some(team) = engine_team(profile) else {
            return false;
        };
        // SAFETY: PeerAuditToken has the exact C layout of audit_token_t and
        // CFData copies these bytes before the local value is dropped.
        let bytes = unsafe {
            slice::from_raw_parts(
                (&token as *const PeerAuditToken).cast::<u8>(),
                size_of::<PeerAuditToken>(),
            )
        };
        let audit = CFData::from_buffer(bytes);
        let mut attributes = GuestAttributes::new();
        attributes.set_audit_token(audit.as_concrete_TypeRef());
        let Ok(code) = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE) else {
            return false;
        };
        let Ok(requirement) = SecRequirement::from_str(&format!(
            "anchor apple generic and identifier \"{}\" and certificate leaf[subject.OU] = \"{team}\"",
            profile.macos_app_id
        )) else {
            return false;
        };
        code.check_validity(
            Flags::STRICT_VALIDATE | Flags::NO_NETWORK_ACCESS,
            &requirement,
        )
        .is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use soyeht_protocol::profile::DEV;
        use std::os::unix::net::UnixStream as StdUnixStream;

        #[test]
        fn team_identifier_is_closed_and_exact() {
            assert_eq!(closed_team("ABCDE12345".into()), Some("ABCDE12345".into()));
            assert_eq!(closed_team("not set".into()), None);
            assert_eq!(closed_team("ABCDE12345-extra".into()), None);
            assert_eq!(closed_team("aBCDE12345".into()), None);
        }

        #[test]
        fn ad_hoc_process_audit_token_is_not_the_signed_dev_ui() {
            let (left, _right) = StdUnixStream::pair().unwrap();
            let token = read_audit_token(left.as_raw_fd()).unwrap();
            assert_eq!(token.words[5], std::process::id());
            assert_ne!(token.words[7], 0, "audit token omitted the PID generation");
            assert!(!signed_ui(token, &DEV));
        }

        #[test]
        fn attested_identifier_matches_every_mac_build_configuration() {
            let project = include_str!("../../../apps/mac/SoyehtMac.xcodeproj/project.pbxproj");
            fn setting<'a>(block: &'a str, key: &str) -> Option<&'a str> {
                let prefix = format!("\"{key}\" = \"");
                block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(&prefix)?.strip_suffix("\";"))
            }

            let configurations: Vec<_> = project
                .split("\n\t\t};")
                .filter(|block| setting(block, "isa") == Some("XCBuildConfiguration"))
                .collect();
            let ui: Vec<_> = configurations
                .iter()
                .filter(|block| setting(block, "SOYEHT_APP_BUNDLE_ID").is_some())
                .collect();
            assert_eq!(
                ui.len(),
                2,
                "the Mac app must have Debug and Release settings"
            );
            for block in ui {
                assert_eq!(
                    setting(block, "SOYEHT_APP_BUNDLE_ID"),
                    Some(DEV.macos_app_id)
                );
                assert_eq!(
                    setting(block, "PRODUCT_BUNDLE_IDENTIFIER"),
                    Some("$(SOYEHT_APP_BUNDLE_ID)")
                );
            }

            let fixture: Vec<_> = configurations
                .iter()
                .filter(|block| setting(block, "TEST_TARGET_NAME") == Some("SoyehtMac"))
                .collect();
            assert_eq!(
                fixture.len(),
                2,
                "the UI test must have Debug and Release settings"
            );
            for block in fixture {
                let identifier = setting(block, "PRODUCT_BUNDLE_IDENTIFIER")
                    .expect("the UI test must have its own bundle identifier");
                assert_ne!(
                    identifier, DEV.macos_app_id,
                    "the fixture must not be an attested UI"
                );
                assert_ne!(identifier, "$(SOYEHT_APP_BUNDLE_ID)");
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{PeerAuditToken, peer_audit_token};

#[cfg(target_os = "macos")]
pub fn signed_ui_peer(token: PeerAuditToken, profile: &soyeht_protocol::profile::Profile) -> bool {
    macos::signed_ui(token, profile)
}

/// Only macOS can attest which signed program is on the other end of a local
/// connection. Elsewhere the token carries no identity: the caller has already
/// proved the peer runs as the engine's own user, the connection is accepted,
/// and no peer is ever treated as a signed UI.
#[cfg(not(target_os = "macos"))]
pub struct PeerAuditToken;

#[cfg(not(target_os = "macos"))]
pub fn peer_audit_token(
    _: &soyeht_platform::local::asynchronous::Stream,
) -> Option<PeerAuditToken> {
    Some(PeerAuditToken)
}

#[cfg(not(target_os = "macos"))]
pub fn signed_ui_peer(_: PeerAuditToken, _: &soyeht_protocol::profile::Profile) -> bool {
    false
}
