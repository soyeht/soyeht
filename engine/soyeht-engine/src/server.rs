use crate::{core::Core, dispatch::Outcome, model, outbox, transport};
use serde_json::Value;
use soyeht_protocol::frame::Frame;
use std::{
    fs::{self, File, OpenOptions},
    io,
    net::{IpAddr, SocketAddr},
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, UnixListener},
    sync::Semaphore,
};

type Shared = Arc<Mutex<Core>>;

pub struct Config {
    pub directory: PathBuf,
    pub listen: Vec<SocketAddr>,
}

pub fn permitted_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn singleton(directory: &Path) -> io::Result<File> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("engine.lock"))?;
    // SAFETY: lock owns a live descriptor retained throughout run().
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::other("engine already running"));
    }
    Ok(lock)
}

pub async fn run(config: Config) -> io::Result<()> {
    if config.listen.iter().any(|a| !permitted_address(a.ip())) {
        return Err(io::Error::other(
            "engine accepts only loopback or tailnet listener addresses",
        ));
    }
    let _lock = singleton(&config.directory)?;
    let core = Arc::new(Mutex::new(
        Core::open(&config.directory).map_err(|e| io::Error::other(e.message))?,
    ));
    let socket = config.directory.join("engine.sock");
    match fs::symlink_metadata(&socket) {
        Ok(m) if m.file_type().is_socket() => fs::remove_file(&socket)?,
        Ok(_) => return Err(io::Error::other("engine socket path is not a socket")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let unix = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let permits = Arc::new(Semaphore::new(128));
    let mut tasks = tokio::task::JoinSet::new();
    for address in config.listen {
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        if !address.ip().is_loopback()
            || core.lock().expect("core").pair_address.starts_with("127.")
        {
            core.lock().expect("core").pair_address = address.to_string();
        }
        let core = Arc::clone(&core);
        let permits = Arc::clone(&permits);
        tasks.spawn(async move {
            loop {
                let (stream, _) = listener.accept().await?;
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let core = Arc::clone(&core);
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok(Ok((reader, writer))) =
                        tokio::time::timeout(Duration::from_secs(5), transport::websocket(stream))
                            .await
                    {
                        connection(core, false, reader, writer).await;
                    }
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), io::Error>(())
        });
    }
    {
        let core = Arc::clone(&core);
        let permits = Arc::clone(&permits);
        tasks.spawn(async move {
            loop {
                let (stream, _) = unix.accept().await?;
                // SAFETY: geteuid has no arguments or memory preconditions.
                let Ok(credential) = stream.peer_cred() else {
                    continue;
                };
                // SAFETY: geteuid has no arguments or memory preconditions.
                if credential.uid() != unsafe { libc::geteuid() } {
                    continue;
                }
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let core = Arc::clone(&core);
                tokio::spawn(async move {
                    let _permit = permit;
                    let (reader, writer) = transport::unix(stream);
                    connection(core, true, reader, writer).await;
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), io::Error>(())
        });
    }
    {
        let core = Arc::clone(&core);
        tasks.spawn(async move {
            let mut ticks=tokio::time::interval(Duration::from_millis(10));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut prune=0u32;let mut previous_error=None;
            loop {
                ticks.tick().await;
                let core=Arc::clone(&core);
                let result=tokio::task::spawn_blocking(move || {
                    let mut core=core.lock().expect("core");
                    let output=core.poll_output();let streams=core.pump_streams();
                    if prune==0 {core.store.prune().map_err(crate::errors::storage)?;}
                    output.and(streams)
                }).await.map_err(io::Error::other)?;
                prune=(prune+1)%6000;
                let error=result.err().map(|e|e.data.kind);
                if error!=previous_error {
                    if let Some(kind)=&error {eprintln!("{}",serde_json::json!({"event":"engine.storage_or_supervisor_error","kind":kind}));}
                    previous_error=error;
                }
            }
            #[allow(unreachable_code)] Ok::<(),io::Error>(())
        });
    }
    println!(
        "{}",
        serde_json::json!({"event":"engine.ready","socket":socket,"version":env!("CARGO_PKG_VERSION")})
    );
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _=tokio::signal::ctrl_c()=>(),
        _=terminate.recv()=>(),
        result=tasks.join_next()=>{if let Some(result)=result {result.map_err(io::Error::other)??;}},
    }
    tasks.abort_all();
    for client in core.lock().expect("core").clients.values() {
        client.outbox.close();
    }
    fs::remove_file(socket)?;
    Ok(())
}

async fn connection(
    core: Shared,
    local: bool,
    mut reader: transport::Reader,
    mut writer: transport::Writer,
) {
    let (outbox, mut receiver) = outbox::channel();
    let id = match core.lock().expect("core").connect(local, outbox.clone()) {
        Ok(id) => id,
        Err(_) => return,
    };
    let mut write_task = tokio::spawn(async move {
        let mut flush = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                packet=receiver.next()=>{
                    let Some(mut packet)=packet else {break};
                    if !packet.is_current() {continue;}
                    match tokio::time::timeout(Duration::from_secs(5),writer.write(&packet.frame)).await {Ok(Ok(()))=>packet.written(),_=>break}
                },
                _=flush.tick()=>{if !matches!(tokio::time::timeout(Duration::from_secs(5),writer.flush()).await,Ok(Ok(()))) {break;}}
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), writer.close()).await;
    });
    let read_loop = async {
        loop {
            let wait = {
                let core = core.lock().expect("core");
                let client = &core.clients[&id];
                if client.actor.is_some() {
                    None
                } else if client.pair_request.is_some() {
                    Some(Duration::from_secs(600))
                } else {
                    Some(Duration::from_secs(5))
                }
            };
            let frame = if let Some(wait) = wait {
                match tokio::time::timeout(wait, reader.next()).await {
                    Ok(result) => result,
                    Err(_) => break,
                }
            } else {
                reader.next().await
            };
            let Ok(Some(Frame::Json(bytes))) = frame else {
                break;
            };
            let Ok(request) = serde_json::from_slice::<Value>(&bytes) else {
                break;
            };
            let started = Instant::now();
            let operation_id = uuid::Uuid::new_v4().to_string();
            let method = request["method"]
                .as_str()
                .and_then(|name| {
                    soyeht_protocol::METHODS
                        .iter()
                        .find(|entry| entry.0 == name)
                        .map(|entry| entry.0)
                })
                .unwrap_or("unknown");
            let request_id = request["id"].clone();
            let failed_handshake = request["method"] == "hello";
            let shared = Arc::clone(&core);
            let connection = id.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                shared.lock().expect("core").dispatch(&connection, request)
            })
            .await;
            let (status, kind) = match &outcome {
                Ok(Ok(Outcome::Reply(_))) => ("reply", None),
                Ok(Ok(Outcome::Queued)) => ("queued", None),
                Ok(Ok(Outcome::Snapshot { .. })) => ("snapshot", None),
                Ok(Err(error)) => ("error", Some(&error.data.kind)),
                Err(_) => ("worker_failed", None),
            };
            eprintln!(
                "{}",
                operation_record(
                    &operation_id,
                    &id,
                    method,
                    status,
                    kind,
                    started.elapsed().as_millis()
                )
            );
            match outcome {
                Ok(Ok(Outcome::Reply(result))) => {
                    outbox.send(model::response(request_id, result));
                }
                Ok(Ok(Outcome::Queued)) => (),
                Ok(Ok(Outcome::Snapshot { parts, result })) => {
                    for part in parts {
                        if outbox.write_page(part).await.is_err() {
                            return;
                        }
                    }
                    outbox.send(model::response(request_id, result));
                }
                Ok(Err(error)) => {
                    if failed_handshake {
                        let _ = outbox.write_page(model::failure(request_id, error)).await;
                        break;
                    }
                    outbox.send(model::failure(request_id, error));
                }
                Err(_) => break,
            }
        }
    };
    tokio::select! {_=read_loop=>(),_=&mut write_task=>()}
    outbox.close();
    write_task.abort();
    let mut core = core.lock().expect("core");
    core.clients.remove(&id);
    core.native_waiters.retain(|_, waiters| {
        waiters.retain(|(connection, _)| connection != &id);
        !waiters.is_empty()
    });
}

fn operation_record(
    operation: &str,
    connection: &str,
    method: &str,
    outcome: &str,
    kind: Option<&soyeht_protocol::ErrorKind>,
    duration_ms: u128,
) -> Value {
    // Only an allowlisted method name is retained. Request IDs and parameters
    // may contain credentials or terminal bytes and never enter this record.
    let method = soyeht_protocol::METHODS
        .iter()
        .find(|entry| entry.0 == method)
        .map(|entry| entry.0)
        .unwrap_or("unknown");
    serde_json::json!({
        "event": "engine.operation",
        "operationId": operation,
        "connectionId": connection,
        "method": method,
        "outcome": outcome,
        "kind": kind,
        "durationMs": duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operation_log_keeps_identity_but_never_unknown_method_text() {
        let operation = uuid::Uuid::new_v4().to_string();
        let connection = uuid::Uuid::new_v4().to_string();
        let record = operation_record(
            &operation,
            &connection,
            "untrusted-private-text",
            "error",
            Some(&soyeht_protocol::ErrorKind::InvalidParams),
            12,
        );
        assert_eq!(record["operationId"], operation);
        assert_eq!(record["connectionId"], connection);
        assert_eq!(record["method"], "unknown");
        assert_eq!(record["kind"], "invalid_params");
        assert!(!record.to_string().contains("untrusted-private-text"));
        let success = operation_record(&operation, &connection, "pane.create", "reply", None, 1);
        assert_eq!(success["method"], "pane.create");
        assert!(success["kind"].is_null());
    }
    #[test]
    fn listener_refuses_public_lan_and_wildcard_addresses() {
        for ip in ["0.0.0.0", "::", "192.168.1.10", "8.8.8.8", "100.128.0.1"] {
            assert!(!permitted_address(ip.parse().unwrap()));
        }
        for ip in [
            "127.0.0.1",
            "::1",
            "100.64.0.1",
            "100.127.255.254",
            "fd7a:115c:a1e0::1",
        ] {
            assert!(permitted_address(ip.parse().unwrap()));
        }
    }
}
