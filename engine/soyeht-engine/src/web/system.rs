use crate::ports::{PortScanError, PortScanObservation, PortScanPort};
use std::{path::PathBuf, time::Duration};
#[cfg(unix)]
use {
    soyeht_protocol::limits,
    soyeht_web::LocalServerScan,
    std::{ffi::OsStr, path::Path, process::Command},
};

/// What one `lsof` run is given on an ordinary host.
pub(super) const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

/// How many times [`SCAN_TIMEOUT`] one run may be given after runs that timed out.
pub(super) const SCAN_PATIENCE: u32 = 3;

pub struct SystemPortScan {
    // Windows has no program to run: its scan reports that listing is unavailable.
    #[cfg_attr(windows, allow(dead_code))]
    program: PathBuf,
    /// What each run is given while the host keeps up.
    #[cfg_attr(windows, allow(dead_code))]
    timeout: Duration,
    /// What the next run is given: `timeout`, or more after a run that timed out. A host
    /// busy enough to outlive `timeout` once usually does so again, and killing the same
    /// slow run every period would leave the list empty for as long as the load lasts.
    #[cfg_attr(windows, allow(dead_code))]
    allowance: Duration,
}

impl SystemPortScan {
    fn with(program: PathBuf, timeout: Duration) -> Self {
        Self {
            program,
            timeout,
            allowance: timeout,
        }
    }

    /// Doubles the allowance, up to `SCAN_PATIENCE` times `timeout`, after a run that timed
    /// out; any other outcome means the host keeps up, and the allowance returns to `timeout`.
    #[cfg(unix)]
    fn learn(&mut self, outcome: &Result<PortScanObservation, PortScanError>) {
        let timed_out = matches!(outcome, Err(PortScanError::Process(error)) if error.timed_out());
        self.allowance = if timed_out {
            (self.allowance * 2).min(self.timeout * SCAN_PATIENCE)
        } else {
            self.timeout
        };
    }
}

impl Default for SystemPortScan {
    fn default() -> Self {
        #[cfg(unix)]
        let program = lsof_program();
        #[cfg(windows)]
        let program = PathBuf::new();
        Self::with(program, SCAN_TIMEOUT)
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
        let outcome = self.observe();
        self.learn(&outcome);
        outcome
    }
}

#[cfg(unix)]
impl SystemPortScan {
    fn observe(&self) -> Result<PortScanObservation, PortScanError> {
        observe_with(|arguments| run_lsof(&self.program, self.allowance, arguments))
    }
}

/// The two observations of one scan, each produced by `run` for the `lsof` arguments given:
/// the listening processes, then the working directory of each. `run` is the seam that lets
/// the tests supply `lsof` output without starting a program.
#[cfg(unix)]
fn observe_with(
    mut run: impl FnMut(&[&str]) -> Result<Vec<u8>, PortScanError>,
) -> Result<PortScanObservation, PortScanError> {
    let listeners = run(&["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-F0pcn"])?;
    let pids = LocalServerScan::listener_pids(&listeners).map_err(PortScanError::Unreadable)?;
    let directories = if pids.is_empty() {
        Vec::new()
    } else {
        let list = pids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        run(&["-nP", "-a", "-p", &list, "-d", "cwd", "-F0pn"])?
    };
    Ok(PortScanObservation {
        listeners,
        directories,
    })
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
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    };
    use uuid::Uuid;

    fn script(body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("soyeht-port-scan-{}", Uuid::new_v4()));
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn scanner(program: PathBuf) -> SystemPortScan {
        SystemPortScan::with(program, Duration::from_secs(10))
    }

    /// What `lsof` left behind: an exit status, or the signal that ended it.
    fn finished(
        status: std::process::ExitStatus,
        stdout: &[u8],
        stderr: &str,
    ) -> soyeht_process::Output {
        soyeht_process::Output {
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn exited(code: i32, stdout: &[u8], stderr: &str) -> soyeht_process::Output {
        finished(ExitStatusExt::from_raw(code << 8), stdout, stderr)
    }

    fn killed(signal: i32) -> soyeht_process::Output {
        finished(ExitStatusExt::from_raw(signal), b"", "")
    }

    /// Scans with `lsof` answering each run with `answer(arguments)`. No program is started,
    /// so the result does not depend on how quickly the host starts one: the first run of a
    /// file written a moment ago waits for the system to assess it, and on a busy macOS host
    /// that wait grows by about half a second for every other new file waiting with it.
    fn scan_answering(
        mut answer: impl FnMut(&[&str]) -> soyeht_process::Output,
    ) -> Result<PortScanObservation, PortScanError> {
        observe_with(|arguments| listing(answer(arguments)))
    }

    /// A system program, which has been assessed already, that prints nothing.
    fn system_program(name: &str) -> PathBuf {
        ["/usr/bin", "/bin"]
            .iter()
            .map(|directory| Path::new(directory).join(name))
            .find(|candidate| is_executable_file(candidate))
            .unwrap()
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
        let observation = scan_answering(|_| exited(1, b"", "")).unwrap();
        assert!(observation.listeners.is_empty());
        assert!(observation.directories.is_empty());
        assert!(
            scan_answering(|_| exited(0, b"", ""))
                .unwrap()
                .listeners
                .is_empty()
        );
    }

    #[test]
    fn a_process_that_vanished_between_the_two_runs_keeps_the_listing() {
        let observation = scan_answering(|arguments| {
            if arguments[2] == "-p" {
                exited(1, b"p17\0fcwd\0n/tmp/site\0", "")
            } else {
                exited(0, b"p17\0cnode\0n127.0.0.1:5173\0", "")
            }
        })
        .unwrap();
        assert!(!observation.directories.is_empty());
    }

    #[test]
    fn a_failed_lsof_is_an_error_not_an_empty_list() {
        let refused = scan_answering(|_| exited(1, b"", "lsof: illegal option")).unwrap_err();
        assert!(
            matches!(&refused, PortScanError::Failed { code: Some(1), detail } if detail.contains("illegal option")),
            "{refused}"
        );
        let crashed = scan_answering(|_| exited(2, b"", "")).unwrap_err();
        assert!(
            matches!(crashed, PortScanError::Failed { code: Some(2), .. }),
            "{crashed}"
        );
        let signalled = scan_answering(|_| killed(9)).unwrap_err();
        assert!(
            matches!(signalled, PortScanError::Failed { code: None, .. }),
            "{signalled}"
        );
    }

    #[test]
    fn a_program_that_runs_is_read_by_its_status() {
        // The one test that starts a program on the success path. Its deadline is far longer
        // than any start it waits for, since the deadline is not what it tests.
        let slow_host = Duration::from_secs(120);
        for name in ["false", "true"] {
            let observation = SystemPortScan::with(system_program(name), slow_host)
                .scan()
                .unwrap();
            assert!(observation.listeners.is_empty(), "{name}");
            assert!(observation.directories.is_empty(), "{name}");
        }
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
        let mut hung = SystemPortScan::with(program.clone(), Duration::from_millis(200));
        let error = hung.scan().unwrap_err();
        fs::remove_file(program).unwrap();
        assert!(
            matches!(&error, PortScanError::Process(inner) if inner.timed_out()),
            "{error}"
        );
    }

    #[test]
    fn a_host_that_outlives_the_deadline_is_given_more_time_until_it_keeps_up() {
        let hanging = script("sleep 30");
        let base = Duration::from_millis(100);
        let mut adapter = SystemPortScan::with(hanging.clone(), base);
        let mut allowances = Vec::new();
        for _ in 0..4 {
            let error = adapter.scan().unwrap_err();
            assert!(
                matches!(&error, PortScanError::Process(inner) if inner.timed_out()),
                "{error}"
            );
            allowances.push(adapter.allowance);
        }
        // Doubled, then held at the patience limit rather than growing without bound.
        assert_eq!(
            allowances,
            [200, 300, 300, 300].map(Duration::from_millis),
            "base {base:?}"
        );
        // A run that finished is the host keeping up: the base deadline returns.
        adapter.learn(&Ok(PortScanObservation {
            listeners: Vec::new(),
            directories: Vec::new(),
        }));
        assert_eq!(adapter.allowance, base);
        fs::remove_file(hanging).unwrap();
    }

    #[test]
    fn a_failure_that_is_not_a_timeout_does_not_widen_the_deadline() {
        let mut adapter = scanner(PathBuf::new());
        adapter.allowance = adapter.timeout * 2;
        adapter.learn(&Err(PortScanError::Failed {
            code: Some(2),
            detail: String::new(),
        }));
        assert_eq!(adapter.allowance, adapter.timeout);
    }

    #[test]
    fn system_adapter_uses_two_bounded_machine_readable_observations() {
        let mut runs: Vec<Vec<String>> = Vec::new();
        let observation = scan_answering(|arguments| {
            runs.push(
                arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            );
            if arguments[2] == "-p" {
                exited(0, b"p17\0fcwd\0n/tmp/site\0", "")
            } else {
                exited(0, b"p17\0cnode\0n127.0.0.1:5173\0", "")
            }
        })
        .unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs[0].contains(&"-sTCP:LISTEN".to_owned()));
        assert_eq!(&runs[1][2..4], ["-p", "17"]);
        assert!(
            runs.iter()
                .all(|run| run.last().is_some_and(|last| last.starts_with("-F0")))
        );
        let scan =
            LocalServerScan::parse_lsof(&observation.listeners, &observation.directories, &[])
                .unwrap();
        assert_eq!(scan.servers()[0].cwd, "/tmp/site");
    }
}
