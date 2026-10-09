use crate::ports::{PortScanError, PortScanObservation, PortScanPort};
use soyeht_protocol::limits;
use soyeht_web::LocalServerScan;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SystemPortScan {
    program: PathBuf,
    timeout: Duration,
}

impl Default for SystemPortScan {
    fn default() -> Self {
        Self {
            program: "/usr/sbin/lsof".into(),
            timeout: SCAN_TIMEOUT,
        }
    }
}

impl PortScanPort for SystemPortScan {
    fn scan(&mut self) -> Result<PortScanObservation, PortScanError> {
        let listeners = run_lsof(
            &self.program,
            self.timeout,
            &["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-F0pcn"],
        )?;
        let pids = LocalServerScan::listener_pids(&listeners).map_err(PortScanError::Unreadable)?;
        let directories = if pids.is_empty() {
            Vec::new()
        } else {
            let list = pids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            run_lsof(
                &self.program,
                self.timeout,
                &["-nP", "-a", "-p", &list, "-d", "cwd", "-F0pn"],
            )?
        };
        Ok(PortScanObservation {
            listeners,
            directories,
        })
    }
}

/// `lsof` exits with 1 when its selection matched nothing, or when some of it vanished
/// while it ran, and prints nothing to standard error for either.
const LSOF_NO_MATCH: i32 = 1;

/// Runs `lsof` off the Core lock with its deadline and byte budget. Empty output is an
/// answer (nothing listens) only when `lsof` itself says so; a failed or unfinished run
/// is an error.
fn run_lsof(
    program: &Path,
    timeout: Duration,
    arguments: &[&str],
) -> Result<Vec<u8>, PortScanError> {
    let mut command = Command::new(program);
    command.args(arguments);
    let output = soyeht_process::run(
        command,
        soyeht_process::Limits::new(timeout, limits::PORT_SCAN_BYTES),
    )
    .map_err(PortScanError::Process)?;
    listing(output)
}

/// The listing in `output`, or why `lsof` did not produce one.
///
/// Status 0 is a listing. Status 1 is one too, possibly empty, unless standard error says
/// why the run failed (a bad option, no permission) and nothing was listed. Any other
/// status, or a signal, is a failure.
fn listing(output: soyeht_process::Output) -> Result<Vec<u8>, PortScanError> {
    let nothing_to_explain = !output.stdout.is_empty() || output.stderr.is_empty();
    if output.success() || (output.status.code() == Some(LSOF_NO_MATCH) && nothing_to_explain) {
        Ok(output.stdout)
    } else {
        Err(PortScanError::Failed {
            code: output.status.code(),
            detail: output.stderr_text(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use uuid::Uuid;

    fn script(body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("soyeht-port-scan-{}", Uuid::new_v4()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn scanner(program: PathBuf) -> SystemPortScan {
        SystemPortScan {
            program,
            timeout: Duration::from_secs(10),
        }
    }

    /// Scans with a stand-in `lsof` running `body`, then removes it.
    fn scan_with(body: &str) -> Result<PortScanObservation, PortScanError> {
        let program = script(body);
        let result = scanner(program.clone()).scan();
        fs::remove_file(program).unwrap();
        result
    }

    #[test]
    fn no_listener_is_an_empty_observation() {
        // Real lsof: status 1, nothing on standard output or standard error.
        let observation = scan_with("exit 1").unwrap();
        assert!(observation.listeners.is_empty());
        assert!(observation.directories.is_empty());
        assert!(scan_with("exit 0").unwrap().listeners.is_empty());
    }

    #[test]
    fn a_process_that_vanished_between_the_two_runs_keeps_the_listing() {
        let observation = scan_with(
            r#"if [ "$3" = "-p" ]; then
  printf 'p17\0fcwd\0n/tmp/site\0'
  exit 1
fi
printf 'p17\0cnode\0n127.0.0.1:5173\0'"#,
        )
        .unwrap();
        assert!(!observation.directories.is_empty());
    }

    #[test]
    fn a_failed_lsof_is_an_error_not_an_empty_list() {
        let refused = scan_with("echo 'lsof: illegal option' >&2; exit 1").unwrap_err();
        assert!(
            matches!(&refused, PortScanError::Failed { code: Some(1), detail } if detail.contains("illegal option")),
            "{refused}"
        );
        let crashed = scan_with("exit 2").unwrap_err();
        assert!(
            matches!(crashed, PortScanError::Failed { code: Some(2), .. }),
            "{crashed}"
        );
        let signalled = scan_with("kill -9 $$").unwrap_err();
        assert!(
            matches!(signalled, PortScanError::Failed { code: None, .. }),
            "{signalled}"
        );
    }

    #[test]
    fn an_lsof_that_cannot_start_or_hangs_is_an_error() {
        let missing = scanner(PathBuf::from("/nonexistent/soyeht-lsof"))
            .scan()
            .unwrap_err();
        assert!(
            matches!(
                missing,
                PortScanError::Process(soyeht_process::Error::Spawn { .. })
            ),
            "{missing}"
        );
        let program = script("sleep 30");
        let mut hung = scanner(program.clone());
        hung.timeout = Duration::from_millis(200);
        let error = hung.scan().unwrap_err();
        fs::remove_file(program).unwrap();
        assert!(
            matches!(&error, PortScanError::Process(inner) if inner.timed_out()),
            "{error}"
        );
    }

    #[test]
    fn system_adapter_uses_two_bounded_machine_readable_observations() {
        let program = script(
            r#"if [ "$3" = "-p" ]; then
  printf 'p17\0fcwd\0n/tmp/site\0'
else
  printf 'p17\0cnode\0n127.0.0.1:5173\0'
fi"#,
        );
        let mut adapter = scanner(program.clone());
        let observation = adapter.scan().unwrap();
        let scan =
            LocalServerScan::parse_lsof(&observation.listeners, &observation.directories, &[])
                .unwrap();
        assert_eq!(scan.servers()[0].cwd, "/tmp/site");
        fs::remove_file(program).unwrap();
    }
}
