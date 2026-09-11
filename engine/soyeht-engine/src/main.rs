use soyeht_engine::server::{self, Config};
use std::{io, net::SocketAddr, path::PathBuf};

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
    if !listen.is_empty() {
        return server::run(Config { directory, listen }).await;
    }
    let addresses = soyeht_engine::network::tailnet_addresses().unwrap_or_default();
    listen.push("127.0.0.1:8291".parse().expect("loopback"));
    for ip in &addresses {
        listen.push(SocketAddr::new(*ip, 8291));
    }
    tokio::select! {
        result = server::run(Config { directory, listen }) => result,
        _ = soyeht_engine::network::wait_for_address_change(addresses) => {
            // The Dev service manager restarts only the engine. The supervisor
            // and all live shells remain available for adoption.
            eprintln!("{}", serde_json::json!({"event":"engine.network_changed"}));
            Ok(())
        }
    }
}
