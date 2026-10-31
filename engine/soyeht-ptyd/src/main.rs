#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_protocol::{
    data_directory::{data_directory, selected_profile},
    profile::{PTYD_LOG, SELECTION_ENV},
};
use soyeht_ptyd::{Error, Result};
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The text a person reads in the terminal or the log: not the `Debug` form.
            eprintln!("soyeht-ptyd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.as_slice() == ["--protocol-version"] {
        println!("{}", soyeht_ptyd::wire::VERSION);
        return Ok(());
    }
    #[cfg(unix)]
    if let [flag, fd] = args.as_slice()
        && flag == "--pipe-guard"
    {
        let fd = fd.parse().map_err(|_| Error::InvalidGuardianFd)?;
        soyeht_ptyd::pipe_guard::run(fd)?;
        return Ok(());
    }
    let directory = match args.as_slice() {
        [] => {
            let profile = selected_profile(env::var_os(SELECTION_ENV).as_deref())?;
            data_directory(profile, soyeht_platform::dirs::data_base())?
        }
        [flag, path] if flag == "--data-dir" => PathBuf::from(path),
        _ => return Err(Error::InvalidArguments),
    };
    // A panic on the main thread ends the supervisor and the service manager starts it again; the
    // report it leaves is what the next window tells the person about (ADR 0127).
    soyeht_platform::crash::install(soyeht_platform::crash::Settings {
        program: soyeht_platform::crash::Program::Supervisor,
        data: directory.clone(),
        log: PTYD_LOG,
    });
    soyeht_ptyd::server::run(&directory)?;
    Ok(())
}
