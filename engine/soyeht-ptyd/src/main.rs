#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.as_slice() == ["--protocol-version"] {
        println!("{}", soyeht_ptyd::wire::VERSION);
        return Ok(());
    }
    if let [flag, fd] = args.as_slice()
        && flag == "--pipe-guard"
    {
        soyeht_ptyd::pipe_guard::run(fd.parse()?)?;
        return Ok(());
    }
    let directory = match args.as_slice() {
        [] => PathBuf::from(env::var("HOME")?).join("Library/Application Support/Soyeht.next/dev"),
        [flag, path] if flag == "--data-dir" => PathBuf::from(path),
        _ => return Err("usage: soyeht-ptyd [--data-dir <isolated-directory>]".into()),
    };
    soyeht_ptyd::server::run(&directory)?;
    Ok(())
}
