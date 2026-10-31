//! The desktop client's update check (ADR 0122, contract 15): fetch the signed manifest, read it
//! with `soyeht_protocol::update` and keep what the window shows about it.
//!
//! The fetch is `curl` through the bounded runner (the engine's usage host does the same), so the
//! client needs no HTTP library. The transport is not trusted: a manifest counts only when a key
//! of the contract signed it. Phase 1 only notifies; the person's browser downloads the package.

use soyeht_platform::log;
use soyeht_process::{Error as ProcessError, Limits};
use soyeht_protocol::update::{Build, Checked, Offer, Outcome, Version, evaluate};
use soyeht_protocol::update_policy::{
    CHECK_TIMEOUT_SECONDS, CONNECT_SECONDS, DEFAULT_CHANNEL, Failure, MANIFEST_MAX_BYTES,
    MANIFEST_URL, MAX_REDIRECTS, PUBLIC_KEYS, PublicKey, USER_AGENT, text,
};
use std::{ffi::OsStr, process::Command, time::Duration};

/// Windows ships `curl.exe` in its system folder, which is on the search path.
#[cfg(windows)]
const CURL: &str = "curl.exe";
#[cfg(not(windows))]
const CURL: &str = "curl";
/// curl's exit code for a body larger than `--max-filesize`.
const CURL_FILE_TOO_BIG: i32 = 63;
/// The runner waits this long past curl's own limit before it kills the process.
const RUNNER_GRACE: Duration = Duration::from_secs(5);

/// This build: the package version, this system and the stable channel; `None` when the version
/// is not three plain numbers (a build with such a version checks nothing).
pub fn running() -> Option<Build<'static>> {
    build_of(env!("CARGO_PKG_VERSION"))
}

/// The build that runs version `version` on this system.
fn build_of(version: &str) -> Option<Build<'static>> {
    Some(Build {
        version: Version::parse(version)?,
        system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        channel: DEFAULT_CHANNEL,
    })
}

/// The arguments of the manifest fetch: HTTPS only, a few redirects, a size and a time limit, and
/// the contract's `User-Agent`. `--disable` comes first, the only place curl reads it, so that no
/// `.curlrc` of the person changes the request or the output.
fn curl_arguments(url: &str) -> Vec<String> {
    let timeout = CHECK_TIMEOUT_SECONDS.to_string();
    let largest = MANIFEST_MAX_BYTES.to_string();
    let (connect, redirects) = (CONNECT_SECONDS.to_string(), MAX_REDIRECTS.to_string());
    [
        "--disable",
        "--silent",
        "--fail",
        "--location",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--max-redirs",
        &redirects,
        "--user-agent",
        USER_AGENT,
        "--connect-timeout",
        &connect,
        "--max-time",
        &timeout,
        "--max-filesize",
        &largest,
        url,
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The text `program` prints for `url`, within the manifest's size and the check's time.
fn fetch_with(program: &OsStr, url: &str) -> Result<String, Failure> {
    let mut command = Command::new(program);
    command.args(curl_arguments(url));
    let limits = Limits::new(
        Duration::from_secs(CHECK_TIMEOUT_SECONDS) + RUNNER_GRACE,
        MANIFEST_MAX_BYTES,
    );
    let output = soyeht_process::run(command, limits).map_err(|error| match error {
        ProcessError::OutputTooLarge { .. } => Failure::TooLarge,
        _ => Failure::Unreachable,
    })?;
    match output.status.code() {
        Some(0) => String::from_utf8(output.stdout).map_err(|_| Failure::BadEnvelope),
        Some(CURL_FILE_TOO_BIG) => Err(Failure::TooLarge),
        _ => Err(Failure::Unreachable),
    }
}

/// The verdict of `fetch` read against `keys`, refusing a manifest below `floor`. A build with
/// no key fetches nothing: it could not trust the answer.
fn check_with(
    build: &Build<'_>,
    keys: &[PublicKey],
    floor: u64,
    fetch: impl FnOnce() -> Result<String, Failure>,
) -> Result<Checked, Failure> {
    if keys.is_empty() {
        return Err(Failure::NotConfigured);
    }
    evaluate(&fetch()?, build, keys, floor)
}

/// Asks for the manifest and reads it; `floor` is the highest sequence this device has read. It
/// blocks for up to the check's time: run it off the window's thread.
pub fn check(floor: u64) -> Result<Checked, Failure> {
    let build = running().ok_or(Failure::UnknownBuild)?;
    check_with(&build, PUBLIC_KEYS, floor, || {
        fetch_with(OsStr::new(CURL), MANIFEST_URL)
    })
}

/// What the last check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// No check has ended yet.
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available(Offer),
    Failed(Failure),
}

/// The window's view of the update check: whether it is on, and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Updates {
    enabled: bool,
    status: Status,
    /// A check is running, whatever the switch did meanwhile: one at a time.
    in_flight: bool,
    /// The highest sequence number read so far; the next manifest may not be below it.
    floor: u64,
    /// The failure the log last said, so a check that fails the same way each day says it once.
    logged: Option<Failure>,
}

impl Updates {
    /// `floor` is the highest sequence number the device has kept from earlier runs.
    pub fn new(enabled: bool, floor: u64) -> Self {
        Self {
            enabled,
            status: Status::Unknown,
            in_flight: false,
            floor,
            logged: None,
        }
    }

    /// The highest sequence number read so far, for the next check and for the device to keep.
    pub fn floor(&self) -> u64 {
        self.floor
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The newer version on offer, if the check is on and the last one found it.
    pub fn offer(&self) -> Option<&Offer> {
        match &self.status {
            Status::Available(offer) if self.enabled => Some(offer),
            _ => None,
        }
    }

    /// What the window says about the check, for a build at `running`; the Mac says the same
    /// (`contracts/vectors/update-status.json`).
    pub fn headline(&self, running: &str) -> String {
        let about = |sentence: &str, version: &str| sentence.replace("{version}", version);
        match (&self.status, self.enabled) {
            (_, false) => text::OFF.to_owned(),
            (Status::Unknown, true) => text::NOT_CHECKED.to_owned(),
            (Status::Checking, true) => text::CHECKING.to_owned(),
            (Status::UpToDate, true) => about(text::UP_TO_DATE, running),
            (Status::Available(offer), true) => about(text::AVAILABLE, &offer.version),
            (Status::Failed(Failure::NotConfigured), true) => text::NOT_CONFIGURED.to_owned(),
            (Status::Failed(_), true) => text::FAILED.to_owned(),
        }
    }

    /// The warning under the headline: this version is no longer supported.
    pub fn warning(&self) -> Option<&'static str> {
        self.offer()
            .filter(|offer| offer.required)
            .map(|_| text::REQUIRED)
    }

    /// The label of the button that opens the package, when there is one on offer.
    pub fn download_label(&self) -> Option<String> {
        self.offer()
            .map(|offer| text::DOWNLOAD.replace("{version}", &offer.version))
    }

    /// Turns the check on or off. Off forgets what it found, so no badge outlives the choice. On
    /// again while a check still runs shows that one: it is not started a second time.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.status = Status::Unknown;
        } else if self.in_flight {
            self.status = Status::Checking;
        }
    }

    /// A check is about to run: false when it is off or one is already running.
    pub fn begin(&mut self) -> bool {
        if !self.enabled || self.in_flight {
            return false;
        }
        self.in_flight = true;
        self.status = Status::Checking;
        true
    }

    /// A check ended. The answer of a check that was turned off meanwhile is dropped.
    pub fn finish(&mut self, result: Result<Checked, Failure>) {
        self.in_flight = false;
        if !self.enabled {
            return;
        }
        match result {
            Ok(Checked { sequence, outcome }) => {
                self.logged = None;
                self.floor = self.floor.max(sequence);
                self.status = match outcome {
                    Outcome::UpToDate => Status::UpToDate,
                    Outcome::Available(offer) => Status::Available(offer),
                };
            }
            Err(failure) => {
                if self.logged != Some(failure) {
                    let (name, fields) = log_line(failure);
                    log::event(name, &fields);
                }
                self.logged = Some(failure);
                self.status = Status::Failed(failure);
            }
        }
    }
}

/// What a check that ended in `failure` writes to the log. A build with no key asks nobody: that is
/// its state, not a failure, so it has an event of its own and no kind.
fn log_line(failure: Failure) -> (&'static str, Vec<(&'static str, &'static str)>) {
    match failure {
        Failure::NotConfigured => ("desktop.update_check_not_configured", Vec::new()),
        other => ("desktop.update_check_failed", vec![("kind", other.as_str())]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_with_no_key_logs_its_state_and_a_failed_check_logs_its_kind() {
        assert_eq!(
            log_line(Failure::NotConfigured),
            ("desktop.update_check_not_configured", vec![])
        );
        assert_eq!(
            log_line(Failure::Unreachable),
            (
                "desktop.update_check_failed",
                vec![("kind", Failure::Unreachable.as_str())]
            )
        );
    }

    const KEY: PublicKey = PublicKey {
        id: "test",
        key: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    };

    fn build() -> Build<'static> {
        Build {
            version: Version::parse("0.1.0").unwrap(),
            system: "linux",
            architecture: "x86_64",
            channel: "stable",
        }
    }

    #[test]
    fn this_build_is_the_package_version_on_this_system() {
        let build = running().unwrap();
        assert_eq!(build.version.to_string(), env!("CARGO_PKG_VERSION"));
        assert_eq!(build.system, std::env::consts::OS);
        assert!(soyeht_protocol::update_policy::ARCHITECTURES.contains(&build.architecture));
    }

    #[test]
    fn a_build_with_no_key_asks_nobody() {
        let result = check_with(&build(), &[], 0, || panic!("fetched without a key"));
        assert_eq!(result, Err(Failure::NotConfigured));
    }

    #[test]
    fn a_fetch_that_fails_is_its_own_failure_and_text_that_is_no_manifest_is_refused() {
        let unreachable = check_with(&build(), &[KEY], 0, || Err(Failure::Unreachable));
        assert_eq!(unreachable, Err(Failure::Unreachable));
        let garbage = check_with(&build(), &[KEY], 0, || {
            Ok("<html>captive portal</html>".into())
        });
        assert_eq!(garbage, Err(Failure::BadEnvelope));
    }

    #[test]
    fn the_fetch_takes_https_only_a_few_redirects_and_a_size_and_time_limit() {
        let arguments = curl_arguments("https://example.test/manifest.json");
        let at = |flag: &str| {
            arguments
                .iter()
                .position(|argument| argument == flag)
                .unwrap()
        };
        assert_eq!(arguments[at("--proto") + 1], "=https");
        assert_eq!(arguments[at("--proto-redir") + 1], "=https");
        assert_eq!(
            arguments[at("--max-time") + 1],
            CHECK_TIMEOUT_SECONDS.to_string()
        );
        assert_eq!(
            arguments[at("--max-filesize") + 1],
            MANIFEST_MAX_BYTES.to_string()
        );
        assert!(arguments.contains(&"--fail".to_owned()));
        // curl reads `--disable` only as its first argument; it keeps `.curlrc` out of the request.
        assert_eq!(arguments[0], "--disable");
        assert_eq!(arguments[at("--user-agent") + 1], USER_AGENT);
        assert_eq!(arguments[at("--max-redirs") + 1], MAX_REDIRECTS.to_string());
        assert_eq!(
            arguments[at("--connect-timeout") + 1],
            CONNECT_SECONDS.to_string()
        );
        assert_eq!(
            arguments.last().unwrap(),
            "https://example.test/manifest.json"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_is_missing_or_fails_is_unreachable_and_one_that_prints_is_read() {
        let url = "https://example.test/manifest.json";
        assert_eq!(
            fetch_with(OsStr::new("/nonexistent/curl"), url),
            Err(Failure::Unreachable)
        );
        assert_eq!(
            fetch_with(OsStr::new("false"), url),
            Err(Failure::Unreachable)
        );
        // `echo` prints its arguments: the text read back is exactly what the program wrote.
        let printed = fetch_with(OsStr::new("echo"), url).unwrap();
        assert!(printed.trim_end().ends_with(url));
    }

    #[test]
    fn the_sentences_are_the_ones_the_vectors_record() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../contracts/vectors/update-status.json"
        ))
        .unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert!(cases.len() >= 8);
        for case in cases {
            let mut updates = Updates::new(case["enabled"].as_bool().unwrap(), 0);
            let status = &case["status"];
            updates.status = match status["kind"].as_str().unwrap() {
                "unknown" => Status::Unknown,
                "checking" => Status::Checking,
                "upToDate" => Status::UpToDate,
                "available" => Status::Available(Offer {
                    version: status["version"].as_str().unwrap().to_owned(),
                    required: status["required"].as_bool().unwrap(),
                    notes_url: String::new(),
                    package: soyeht_protocol::update::Package {
                        url: String::new(),
                        sha256: String::new(),
                        size: 1,
                    },
                }),
                "failed" => Status::Failed(
                    [
                        Failure::NotConfigured,
                        Failure::Unreachable,
                        Failure::BadSignature,
                    ]
                    .into_iter()
                    .find(|failure| failure.as_str() == status["failure"].as_str().unwrap())
                    .unwrap(),
                ),
                other => panic!("{other}"),
            };
            let expected = &case["expected"];
            let name = case["name"].as_str().unwrap();
            assert_eq!(
                updates.headline(case["running"].as_str().unwrap()),
                expected["headline"].as_str().unwrap(),
                "{name}"
            );
            assert_eq!(updates.warning(), expected["warning"].as_str(), "{name}");
            assert_eq!(
                updates.download_label().as_deref(),
                expected["download"].as_str(),
                "{name}"
            );
            assert_eq!(
                updates.offer().is_some(),
                expected["badge"].as_bool().unwrap(),
                "{name}"
            );
        }
    }

    fn checked(outcome: Outcome, sequence: u64) -> Checked {
        Checked { sequence, outcome }
    }

    #[test]
    fn a_version_that_is_not_three_plain_numbers_makes_no_build() {
        // `check` ends such a build as `UnknownBuild`, not as a fault of the manifest.
        for bad in ["", "0.1", "0.1.0-beta", "01.2.3"] {
            assert!(build_of(bad).is_none(), "{bad}");
        }
        assert!(build_of("0.1.0").is_some());
    }

    #[test]
    fn turning_the_check_off_and_on_while_one_runs_keeps_it_one_check() {
        let mut updates = Updates::new(true, 0);
        assert!(updates.begin());
        updates.set_enabled(false);
        updates.set_enabled(true);
        assert!(!updates.begin(), "a second check started beside the first");
        assert_eq!(updates.status(), &Status::Checking);
        updates.finish(Ok(checked(Outcome::UpToDate, 3)));
        assert_eq!(updates.status(), &Status::UpToDate);
        assert!(updates.begin());
    }

    #[test]
    fn the_answer_of_a_check_turned_off_meanwhile_is_dropped_and_frees_the_next() {
        let mut updates = Updates::new(true, 0);
        assert!(updates.begin());
        updates.set_enabled(false);
        updates.finish(Ok(checked(Outcome::UpToDate, 9)));
        assert_eq!(updates.status(), &Status::Unknown);
        assert_eq!(updates.floor(), 0);
        updates.set_enabled(true);
        assert!(updates.begin());
    }

    #[test]
    fn the_floor_only_rises_with_what_was_read() {
        let mut updates = Updates::new(true, 5);
        assert_eq!(updates.floor(), 5);
        assert!(updates.begin());
        updates.finish(Ok(checked(Outcome::UpToDate, 9)));
        assert_eq!(updates.floor(), 9);
        assert!(updates.begin());
        updates.finish(Ok(checked(Outcome::UpToDate, 7)));
        assert_eq!(updates.floor(), 9);
        assert!(updates.begin());
        updates.finish(Err(Failure::StaleManifest));
        assert_eq!(updates.floor(), 9);
    }

    #[test]
    fn a_check_runs_once_at_a_time_and_only_while_it_is_on() {
        let mut updates = Updates::new(true, 0);
        assert!(updates.begin());
        assert!(!updates.begin());
        updates.finish(Ok(checked(Outcome::UpToDate, 1)));
        assert_eq!(updates.status(), &Status::UpToDate);
        assert!(updates.begin());
        updates.set_enabled(false);
        assert!(!updates.begin());
        updates.finish(Err(Failure::Unreachable));
        assert_eq!(updates.status(), &Status::Unknown);
    }

    #[test]
    fn an_offer_shows_until_the_check_is_turned_off_or_finds_nothing_newer() {
        let offer = Offer {
            version: "0.2.0".into(),
            required: false,
            notes_url: "https://github.com/soyeht/soyeht/releases/tag/v0.2.0".into(),
            package: soyeht_protocol::update::Package {
                url: "https://github.com/soyeht/soyeht/releases/download/v0.2.0/x.zip".into(),
                sha256: "0".repeat(64),
                size: 1,
            },
        };
        let mut updates = Updates::new(true, 0);
        updates.finish(Ok(checked(Outcome::Available(offer.clone()), 1)));
        assert_eq!(updates.offer(), Some(&offer));
        updates.finish(Err(Failure::Unreachable));
        assert_eq!(updates.offer(), None);
        updates.finish(Ok(checked(Outcome::Available(offer.clone()), 1)));
        updates.set_enabled(false);
        assert_eq!(updates.offer(), None);
    }
}
