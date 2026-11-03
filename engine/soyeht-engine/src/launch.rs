//! The command line of an engine program: where its data lives and where it listens.
//!
//! `soyeht-engine` runs it with [`NoHooks`](crate::hooks::NoHooks); the test program runs it
//! with the hooks of [`crate::test_hooks`].

use crate::{
    errors::{Error, Result},
    hooks::Hooks,
    log_rotation,
    server::{self, Config},
};
use soyeht_platform::log::rotating::{KEEP_ROTATED, MAX_BYTES};
use soyeht_protocol::{
    data_directory::{RESTART_SUPERVISOR_FLAG, resolve},
    profile::{ENGINE_LOG, SELECTION_ENV},
};
use std::{ffi::OsString, net::SocketAddr, path::Path, path::PathBuf, sync::Arc};

/// Runs the engine until it stops. `hooks` receives the data directory the arguments chose.
pub async fn run(hooks: impl FnOnce(&Path) -> Arc<dyn Hooks>) -> Result<()> {
    // Bound the log the launch agent points stderr at; kept until `run` ends.
    let _log_rotation = match log_rotation::capture_stderr(MAX_BYTES, KEEP_ROTATED) {
        Ok(rotation) => rotation,
        Err(error) => {
            soyeht_platform::log::failure("engine.log_rotation_unavailable", &error);
            None
        }
    };
    // Captures record the engine build; hashing the executable overlaps the rest of startup.
    crate::engine_build::prefetch_engine_build();
    let Arguments {
        requested,
        directory,
        listen,
        supervisor_program,
    } = parse_arguments(std::env::args().skip(1), std::env::var_os(SELECTION_ENV))?;
    let (profile, directory) = resolve(
        requested.as_deref(),
        directory,
        soyeht_platform::dirs::data_base(),
    )?;
    // A panic on the main thread ends the engine and the service manager starts it again; the
    // report it leaves is what the next window tells the person about (ADR 0127).
    soyeht_platform::crash::install(soyeht_platform::crash::Settings {
        program: soyeht_platform::crash::Program::Engine,
        data: directory.clone(),
        log: ENGINE_LOG,
    });
    let hooks = hooks(&directory);
    if !listen.is_empty() {
        return server::run(Config {
            directory,
            profile,
            listen,
            tailnet_port: None,
            supervisor_program,
            hooks,
        })
        .await;
    }
    // Local clients always have loopback. Tailnet listeners follow the interfaces
    // inside the engine (ADR 0078): a missing address is logged, reported to clients
    // and retried, and a late address starts listening without a restart.
    server::run(Config {
        directory,
        profile,
        listen: vec![SocketAddr::from(([127, 0, 0, 1], profile.tcp_port))],
        tailnet_port: Some(profile.tcp_port),
        supervisor_program,
        hooks,
    })
    .await
}

/// What the command line says. `requested` starts as the profile the environment names.
struct Arguments {
    requested: Option<OsString>,
    directory: Option<PathBuf>,
    listen: Vec<SocketAddr>,
    supervisor_program: Option<PathBuf>,
}

/// The value after an option, or `missing` when the command line ends.
fn value(args: &mut impl Iterator<Item = String>, missing: Error) -> Result<String> {
    args.next().ok_or(missing)
}

fn parse_arguments(
    arguments: impl Iterator<Item = String>,
    requested: Option<OsString>,
) -> Result<Arguments> {
    let mut parsed = Arguments {
        requested,
        directory: None,
        listen: vec![],
        supervisor_program: None,
    };
    let mut args = arguments;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // A service definition names the profile on the command line, since a Windows
            // scheduled task cannot set an environment variable (ADR 0114).
            "--profile" => {
                parsed.requested = Some(value(&mut args, Error::MissingProfileArgument)?.into())
            }
            "--data-dir" => {
                parsed.directory =
                    Some(value(&mut args, Error::MissingDataDirectoryArgument)?.into())
            }
            "--listen" => parsed.listen.push(
                value(&mut args, Error::MissingListenAddressArgument)?
                    .parse::<SocketAddr>()
                    .map_err(Error::Address)?,
            ),
            RESTART_SUPERVISOR_FLAG => {
                let program =
                    PathBuf::from(value(&mut args, Error::MissingSupervisorProgramArgument)?);
                // A relative path would be read from wherever this engine happens to run.
                if !program.is_absolute() {
                    return Err(Error::InvalidArguments);
                }
                parsed.supervisor_program = Some(program);
            }
            _ => return Err(Error::InvalidArguments),
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str], environment: Option<&str>) -> Result<Arguments> {
        parse_arguments(
            arguments.iter().map(|argument| (*argument).to_owned()),
            environment.map(OsString::from),
        )
    }

    #[test]
    fn the_profile_argument_beats_the_environment() {
        let parsed = parse(&["--profile", "prod"], Some("dev")).unwrap();
        assert_eq!(parsed.requested, Some(OsString::from("prod")));
        let parsed = parse(&["--data-dir", "/d"], Some("dev")).unwrap();
        assert_eq!(parsed.requested, Some(OsString::from("dev")));
        assert_eq!(parsed.directory, Some(PathBuf::from("/d")));
    }

    #[test]
    fn the_arguments_a_service_definition_passes_are_read() {
        let parsed = parse(
            &[
                "--profile",
                "prod",
                "--data-dir",
                "/d",
                RESTART_SUPERVISOR_FLAG,
                "/p/ptyd",
            ],
            None,
        )
        .unwrap();
        assert_eq!(parsed.supervisor_program, Some(PathBuf::from("/p/ptyd")));
        assert!(parsed.listen.is_empty());
    }

    #[test]
    fn a_missing_value_or_an_unknown_argument_is_refused() {
        assert!(matches!(
            parse(&["--profile"], None),
            Err(Error::MissingProfileArgument)
        ));
        assert!(matches!(
            parse(&["--data-dir"], None),
            Err(Error::MissingDataDirectoryArgument)
        ));
        assert!(matches!(
            parse(&[RESTART_SUPERVISOR_FLAG, "relative/ptyd"], None),
            Err(Error::InvalidArguments)
        ));
        assert!(matches!(
            parse(&["--other"], None),
            Err(Error::InvalidArguments)
        ));
    }
}
