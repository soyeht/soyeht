#[cfg(target_os = "macos")]
mod macos {
    use soyeht_protocol::profile::DEV_ENGINE_LAUNCHD_LABEL;
    use std::{
        io::{self, Read},
        os::fd::AsRawFd,
        process::{ChildStderr, Command, ExitStatus, Stdio},
        sync::OnceLock,
        thread,
        time::{Duration, Instant},
    };

    const CODESIGN: &str = "/usr/bin/codesign";
    const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
    const OUTPUT_LIMIT: usize = 64 * 1024;
    const UI_IDENTIFIER: &str = "com.soyeht.next.dev.mac";
    static ENGINE_TEAM: OnceLock<Option<String>> = OnceLock::new();

    fn set_nonblocking(stderr: &ChildStderr) -> io::Result<()> {
        let descriptor = stderr.as_raw_fd();
        // SAFETY: descriptor is owned by `stderr` for both checked calls.
        let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_SETFL changes only the live descriptor's status flags.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_available(stderr: &mut ChildStderr, bytes: &mut Vec<u8>) -> io::Result<bool> {
        let mut buffer = [0_u8; 4096];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) => return Ok(true),
                Ok(count) => {
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.len() > OUTPUT_LIMIT {
                        return Ok(false);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn run_codesign(arguments: &[&str]) -> Option<(ExitStatus, Vec<u8>)> {
        let mut child = Command::new(CODESIGN)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let Some(mut stderr) = child.stderr.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        if set_nonblocking(&stderr).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let mut bytes = Vec::new();
        let mut status = None;
        let mut eof = false;
        while Instant::now() < deadline {
            eof |= read_available(&mut stderr, &mut bytes).ok()?;
            if bytes.len() > OUTPUT_LIMIT {
                break;
            }
            if status.is_none() {
                status = child.try_wait().ok()?;
            }
            if status.is_some() && eof {
                return status.map(|status| (status, bytes));
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        None
    }

    fn signed_team(details: &[u8]) -> Option<String> {
        let details = std::str::from_utf8(details).ok()?;
        let team = details
            .lines()
            .find_map(|line| line.strip_prefix("TeamIdentifier="))?;
        (team.len() == 10
            && team
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
        .then(|| team.to_owned())
    }

    fn verifies(pid: u32, requirement: &str) -> bool {
        let pid = pid.to_string();
        let requirement = format!("-R={requirement}");
        run_codesign(&["-v", "--strict", &requirement, &pid])
            .is_some_and(|(status, _)| status.success())
    }

    fn engine_team() -> Option<&'static str> {
        ENGINE_TEAM
            .get_or_init(|| {
                let pid = std::process::id().to_string();
                let (status, details) = run_codesign(&["-dvvv", &pid])?;
                if !status.success() {
                    return None;
                }
                let requirement =
                    format!("anchor apple generic and identifier \"{DEV_ENGINE_LAUNCHD_LABEL}\"");
                verifies(std::process::id(), &requirement)
                    .then(|| signed_team(&details))
                    .flatten()
            })
            .as_deref()
    }

    pub fn signed_ui(pid: u32) -> bool {
        let Some(team) = engine_team() else {
            return false;
        };
        let requirement = format!(
            "anchor apple generic and identifier \"{UI_IDENTIFIER}\" and certificate leaf[subject.OU] = \"{team}\""
        );
        verifies(pid, &requirement)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn team_identifier_is_closed_and_exact() {
            assert_eq!(
                signed_team(b"Identifier=dev\nTeamIdentifier=ABCDE12345\n"),
                Some("ABCDE12345".into())
            );
            assert_eq!(signed_team(b"TeamIdentifier=not set\n"), None);
            assert_eq!(signed_team(b"TeamIdentifier=ABCDE12345-extra\n"), None);
            assert_eq!(signed_team(b"TeamIdentifier=ABCDE12345\n"), None);
        }

        #[test]
        fn ad_hoc_test_process_is_not_the_signed_dev_ui() {
            assert!(!signed_ui(std::process::id()));
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

pub fn signed_ui_peer(pid: Option<i32>) -> bool {
    #[cfg(target_os = "macos")]
    {
        pid.and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(macos::signed_ui)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        false
    }
}
