use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let directory = match args.as_slice() {
        [] => PathBuf::from(env::var("HOME")?).join("Library/Application Support/Soyeht.next/dev"),
        [flag, path] if flag == "--data-dir" => PathBuf::from(path),
        _ => return Err("usage: soyeht-ptyd [--data-dir <isolated-directory>]".into()),
    };
    soyeht_ptyd::server::run(&directory)?;
    Ok(())
}
