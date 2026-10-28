#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use soyeht_protocol::{
    data_directory::{data_directory, selected_profile},
    profile::SELECTION_ENV,
};
use soyeht_ptyd::{Error, Result};
use std::{env, path::PathBuf};

fn main() -> Result<()> {
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
    soyeht_ptyd::server::run(&directory)?;
    Ok(())
}
