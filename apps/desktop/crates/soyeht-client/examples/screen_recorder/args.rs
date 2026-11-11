//! The command line: which engine to connect to, which panes to record, where the recordings go
//! and how much disk they may take.

use crate::store::Limits;
use soyeht_protocol::{data_directory, profile::ENGINE_SOCKET};
use std::{ffi::OsString, path::PathBuf};

const MEGABYTE: u64 = 1024 * 1024;
const DEFAULT_FILE_MB: u64 = 16;
const DEFAULT_KEEP: usize = 3;
const DEFAULT_TOTAL_MB: u64 = 256;

pub(crate) const USAGE: &str = "\
Records the screens of terminal panes of a running Soyeht engine, for research on when a person
is typing. It records what the screen shows, never keystrokes. Development only; it is not part
of any package. See docs/research/terminal-screen-recorder.md.

usage: cargo run -p soyeht-client --example screen_recorder -- [options]

engine (the first of these that is given wins; none means the engine of this build's profile):
  --socket PATH       the engine's socket
  --data-dir DIR      the data directory of the engine (its socket is DIR/engine.sock)
  --profile NAME      the data directory of a profile (dev or prod) in the system's place
panes:
  --pane ID_OR_NAME   record only this pane (repeat for several); default: every terminal pane
output:
  --out DIR           where the recordings go (default: the research cache folder of this user)
  --max-file-mb N     a pane's file is rotated when it reaches N megabytes (default 16)
  --keep N            rotated files kept per pane (default 3)
  --max-total-mb N    stop writing when the files of this run hold N megabytes (default 256)
Ctrl-C stops the recorder; every record is written whole as it happens.";

#[derive(Debug)]
pub(crate) struct Options {
    pub socket: PathBuf,
    pub panes: Vec<String>,
    pub out: PathBuf,
    pub limits: Limits,
}

/// What the command line asked for.
#[derive(Debug)]
pub(crate) enum Parsed {
    Run(Box<Options>),
    Help,
}

pub(crate) fn parse(
    arguments: impl IntoIterator<Item = OsString>,
    default_out: Option<PathBuf>,
) -> Result<Parsed, String> {
    let mut arguments = arguments.into_iter();
    let mut socket = None;
    let mut data_dir = None;
    let mut profile = None;
    let mut panes = Vec::new();
    let mut out = None;
    let mut limits = Limits {
        file_bytes: DEFAULT_FILE_MB * MEGABYTE,
        keep: DEFAULT_KEEP,
        total_bytes: DEFAULT_TOTAL_MB * MEGABYTE,
    };
    while let Some(argument) = arguments.next() {
        let name = argument.to_string_lossy().into_owned();
        if name == "--help" || name == "-h" {
            return Ok(Parsed::Help);
        }
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match name.as_str() {
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--data-dir" => data_dir = Some(PathBuf::from(value()?)),
            "--profile" => profile = Some(value()?),
            "--pane" => panes.push(value()?.to_string_lossy().into_owned()),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--max-file-mb" => limits.file_bytes = megabytes(&name, &value()?)?,
            "--max-total-mb" => limits.total_bytes = megabytes(&name, &value()?)?,
            "--keep" => limits.keep = count(&name, &value()?)?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let socket = match socket {
        Some(socket) => socket,
        None => {
            let (_, directory) = data_directory::resolve(
                profile.as_deref(),
                data_dir,
                soyeht_platform::dirs::data_base(),
            )
            .map_err(|error| error.to_string())?;
            directory.join(ENGINE_SOCKET)
        }
    };
    let out = out
        .or(default_out)
        .ok_or("the system does not say where this user's cache folder is: give --out")?;
    Ok(Parsed::Run(Box::new(Options {
        socket,
        panes,
        out,
        limits,
    })))
}

fn count(name: &str, value: &OsString) -> Result<usize, String> {
    value
        .to_string_lossy()
        .parse()
        .map_err(|_| format!("{name} needs a whole number"))
}

fn megabytes(name: &str, value: &OsString) -> Result<u64, String> {
    let number: u64 = count(name, value)?
        .try_into()
        .map_err(|_| format!("{name} is too large"))?;
    if number == 0 {
        return Err(format!("{name} must be at least 1"));
    }
    number
        .checked_mul(MEGABYTE)
        .ok_or_else(|| format!("{name} is too large"))
}

/// The folder recordings go to unless `--out` says otherwise: the research folder of the user's
/// cache (`~/Library/Caches` on a Mac), which no package and no part of the app ever reads.
pub(crate) fn default_out() -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    let cache = if cfg!(target_os = "macos") {
        home.join("Library").join("Caches")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map_or_else(|| home.join("AppData").join("Local"), Into::into)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".cache"))
    };
    Some(cache.join("soyeht-research").join("terminal-screens"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arguments: &[&str]) -> Result<Options, String> {
        match parse(
            arguments.iter().map(OsString::from),
            Some(PathBuf::from("/cache/screens")),
        )? {
            Parsed::Run(options) => Ok(*options),
            Parsed::Help => Err("help".into()),
        }
    }

    #[test]
    fn the_socket_is_given_directly_or_found_in_a_data_directory() {
        let direct = run(&["--socket", "/run/e.sock", "--data-dir", "/ignored"]).unwrap();
        assert_eq!(direct.socket, PathBuf::from("/run/e.sock"));
        let by_directory = run(&["--data-dir", "/data/soyeht"]).unwrap();
        assert_eq!(
            by_directory.socket,
            PathBuf::from("/data/soyeht/engine.sock")
        );
    }

    #[test]
    fn a_profile_names_the_data_directory_and_an_unknown_one_is_refused() {
        let prod = run(&["--profile", "prod"]).unwrap();
        assert!(prod.socket.ends_with("engine.sock"));
        assert!(run(&["--profile", "nope"]).is_err());
    }

    #[test]
    fn panes_and_limits_are_read_and_a_bad_value_is_refused() {
        let options = run(&[
            "--data-dir",
            "/d",
            "--pane",
            "a",
            "--pane",
            "b",
            "--max-file-mb",
            "2",
            "--keep",
            "5",
            "--max-total-mb",
            "9",
        ])
        .unwrap();
        assert_eq!(options.panes, ["a", "b"]);
        assert_eq!(options.limits.file_bytes, 2 * MEGABYTE);
        assert_eq!(options.limits.keep, 5);
        assert_eq!(options.limits.total_bytes, 9 * MEGABYTE);
        assert_eq!(options.out, PathBuf::from("/cache/screens"));
        assert!(run(&["--data-dir", "/d", "--max-file-mb", "0"]).is_err());
        assert!(run(&["--data-dir", "/d", "--keep", "many"]).is_err());
        assert!(run(&["--data-dir"]).is_err());
        assert!(run(&["--wat"]).is_err());
    }
}
