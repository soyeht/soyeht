use soyeht_engine::server::{self, Config};
use std::{io, net::SocketAddr, path::PathBuf, process::Command};

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut directory = PathBuf::from(
        std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is required"))?,
    )
    .join("Library/Application Support/Soyeht.next/dev");
    let mut listen = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                directory = PathBuf::from(
                    args.next()
                        .ok_or_else(|| io::Error::other("--data-dir requires a path"))?,
                )
            }
            "--listen" => listen.push(
                args.next()
                    .ok_or_else(|| io::Error::other("--listen requires an address"))?
                    .parse::<SocketAddr>()
                    .map_err(io::Error::other)?,
            ),
            _ => return Err(io::Error::other("expected --data-dir or --listen")),
        }
    }
    if listen.is_empty() {
        listen.push("127.0.0.1:8291".parse().expect("loopback"));
        for cli in [
            "/opt/homebrew/bin/tailscale",
            "/usr/local/bin/tailscale",
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ] {
            if !std::path::Path::new(cli).is_file() {
                continue;
            }
            if let Ok(output) = Command::new(cli).args(["ip", "--4"]).output()
                && output.status.success()
                && let Ok(ip) = String::from_utf8_lossy(&output.stdout).trim().parse()
                && server::permitted_address(ip)
            {
                listen.push(SocketAddr::new(ip, 8291));
            }
            break;
        }
    }
    server::run(Config { directory, listen }).await
}
