use crate::ports::{PortScanError, PortScanObservation, PortScanPort};
use std::{path::PathBuf, time::Duration};
#[cfg(unix)]
use {
    soyeht_protocol::limits,
    soyeht_web::LocalServerScan,
    std::{ffi::OsStr, path::Path, process::Command},
};

const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct SystemPortScan {
    // Windows has no program to run: its scan reports that listing is unavailable.
    #[cfg_attr(windows, allow(dead_code))]
    program: PathBuf,
    #[cfg_attr(windows, allow(dead_code))]
    timeout: Duration,
}

impl Default for SystemPortScan {
    fn default() -> Self {
        Self {
            #[cfg(unix)]
            program: lsof_program(),
            #[cfg(windows)]
            program: PathBuf::new(),
            timeout: SCAN_TIMEOUT,
        }
    }
}

/// Where `lsof` lives when `PATH` does not say: `/usr/sbin` on macOS and many Linux
/// distributions, which a minimal `PATH` such as `/usr/bin:/bin` leaves out.
#[cfg(unix)]
const LSOF_FALLBACKS: [&str; 2] = ["/usr/sbin/lsof", "/usr/bin/lsof"];

/// The `lsof` to run: the first on `PATH`, else the first standard place that has one, else
/// the bare name, which the run then reports as a program that cannot start.
#[cfg(unix)]
pub(crate) fn lsof_program() -> PathBuf {
    find_lsof(std::env::var_os("PATH").as_deref(), &LSOF_FALLBACKS)
}

#[cfg(unix)]
fn find_lsof(path: Option<&OsStr>, fallbacks: &[&str]) -> PathBuf {
    let on_path = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join("lsof"));
    on_path
        .chain(fallbacks.iter().map(PathBuf::from))
        .find(|candidate| is_executable_file(candidate))
        .unwrap_or_else(|| "lsof".into())
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Windows has no `lsof`, and the working directory of another process is not readable
/// through a supported interface, so the listing of local servers is not available.
#[cfg(windows)]
impl PortScanPort for SystemPortScan {
    fn unavailable(&self) -> Option<&'static str> {
        Some(UNAVAILABLE)
    }

    fn scan(&mut self) -> Result<PortScanObservation, PortScanError> {
        Err(PortScanError::Unsupported(UNAVAILABLE))
    }
}

#[cfg(windows)]
const UNAVAILABLE: &str = "Listing local servers is not available on Windows";

#[cfg(unix)]
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

#[cfg(unix)]
/// `lsof` exits with 1 when its selection matched nothing, or when some of it vanished
/// while it ran, and prints nothing to standard error for either.
const LSOF_NO_MATCH: i32 = 1;

/// Runs `lsof` off the Core lock with its deadline and byte budget. Empty output is an
/// answer (nothing listens) only when `lsof` itself says so; a failed or unfinished run
/// is an error.
#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(all(test, unix))]
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

    fn directory_with(name: &str, mode: u32) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("soyeht-lsof-find-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join(name);
        fs::write(&file, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        directory
    }

    #[test]
    fn lsof_is_found_on_path_first() {
        let first = directory_with("lsof", 0o700);
        let second = directory_with("lsof", 0o700);
        let path = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(find_lsof(Some(&path), &[]), first.join("lsof"));
        fs::remove_dir_all(first).unwrap();
        fs::remove_dir_all(second).unwrap();
    }

    #[test]
    fn a_path_without_lsof_falls_back_to_the_standard_places() {
        let standard = directory_with("lsof", 0o700);
        let wanted = standard.join("lsof");
        let empty = directory_with("other", 0o700);
        let path = std::env::join_paths([&empty]).unwrap();
        let missing = std::env::temp_dir().join(format!("soyeht-lsof-none-{}", Uuid::new_v4()));
        let fallbacks = [
            missing.join("lsof").to_str().unwrap().to_owned(),
            wanted.to_str().unwrap().to_owned(),
        ];
        let fallbacks: Vec<&str> = fallbacks.iter().map(String::as_str).collect();
        assert_eq!(find_lsof(Some(&path), &fallbacks), wanted);
        assert_eq!(find_lsof(None, &fallbacks), wanted);
        fs::remove_dir_all(standard).unwrap();
        fs::remove_dir_all(empty).unwrap();
    }

    #[test]
    fn a_file_that_is_not_executable_is_not_lsof() {
        let plain = directory_with("lsof", 0o600);
        let path = std::env::join_paths([&plain]).unwrap();
        assert_eq!(find_lsof(Some(&path), &[]), PathBuf::from("lsof"));
        fs::remove_dir_all(plain).unwrap();
    }

    #[test]
    fn no_lsof_anywhere_is_the_bare_name() {
        assert_eq!(find_lsof(Some(OsStr::new("")), &[]), PathBuf::from("lsof"));
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
