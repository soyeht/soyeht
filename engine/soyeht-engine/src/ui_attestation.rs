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
    use soyeht_protocol::profile::DEV_ENGINE_LAUNCHD_LABEL;
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
    const UI_IDENTIFIER: &str = "com.soyeht.next.dev.mac";
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

    fn engine_team() -> Option<&'static str> {
        ENGINE_TEAM
            .get_or_init(|| {
                let code = SecCode::for_self(Flags::NONE).ok()?;
                let requirement = SecRequirement::from_str(&format!(
                    "anchor apple generic and identifier \"{DEV_ENGINE_LAUNCHD_LABEL}\""
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

    pub fn signed_ui(token: PeerAuditToken) -> bool {
        let Some(team) = engine_team() else {
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
            "anchor apple generic and identifier \"{UI_IDENTIFIER}\" and certificate leaf[subject.OU] = \"{team}\""
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
        use std::os::unix::net::UnixStream as StdUnixStream;

        #[test]
        fn team_identifier_is_closed_and_exact() {
            assert_eq!(closed_team("ABCDE12345".into()), Some("ABCDE12345".into()));
            assert_eq!(closed_team("not set".into()), None);
            assert_eq!(closed_team("ABCDE12345-extra".into()), None);
            assert_eq!(closed_team("ABCDE12345".into()), None);
        }

        #[test]
        fn ad_hoc_process_audit_token_is_not_the_signed_dev_ui() {
            let (left, _right) = StdUnixStream::pair().unwrap();
            let token = read_audit_token(left.as_raw_fd()).unwrap();
            assert_eq!(token.words[5], std::process::id());
            assert_ne!(token.words[7], 0, "audit token omitted the PID generation");
            assert!(!signed_ui(token));
        }

        #[test]
        fn attested_identifier_matches_every_mac_build_configuration() {
            let project = include_str!("../../../apps/mac/SoyehtMac.xcodeproj/project.pbxproj");
            assert_eq!(
                project
                    .lines()
                    .filter(|line| line.contains("PRODUCT_BUNDLE_IDENTIFIER"))
                    .count(),
                2
            );
            assert_eq!(
                project
                    .lines()
                    .filter(|line| {
                        line.contains("PRODUCT_BUNDLE_IDENTIFIER") && line.contains(UI_IDENTIFIER)
                    })
                    .count(),
                2
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{PeerAuditToken, peer_audit_token};

#[cfg(target_os = "macos")]
pub fn signed_ui_peer(token: PeerAuditToken) -> bool {
    macos::signed_ui(token)
}

#[cfg(not(target_os = "macos"))]
pub struct PeerAuditToken;

#[cfg(not(target_os = "macos"))]
pub fn peer_audit_token(_: &tokio::net::UnixStream) -> Option<PeerAuditToken> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn signed_ui_peer(_: PeerAuditToken) -> bool {
    false
}
