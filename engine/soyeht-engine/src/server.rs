use crate::{
    core::Core,
    dispatch::Outcome,
    errors::{self, Error},
    model,
    outbox::{self, CloseReason, Outbox},
    transport, ui_attestation,
};
use serde_json::Value;
use soyeht_protocol::{
    AttachmentRef, attachment::attachment_frames, frame::Frame, profile::DEV_ENGINE_SOCKET,
};
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
    net::{TcpListener, UnixListener, UnixStream},
    sync::Semaphore,
};

type Shared = Arc<Mutex<Core>>;

fn lock_core(shared: &Shared) -> crate::errors::Result<std::sync::MutexGuard<'_, Core>> {
    shared.lock().map_err(|_| Error::EngineStatePoisoned)
}

fn update_pair_address(shared: &Shared, address: SocketAddr) -> crate::errors::Result<()> {
    let mut state = lock_core(shared)?;
    if !address.ip().is_loopback() || state.pair_address.starts_with("127.") {
        state.pair_address = address.to_string();
    }
    Ok(())
}

fn local_peer_audit_token(stream: &UnixStream) -> Option<ui_attestation::PeerAuditToken> {
    let credential = stream.peer_cred().ok()?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    if credential.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    ui_attestation::peer_audit_token(stream)
}

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

fn singleton(directory: &Path) -> crate::errors::Result<File> {
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
        return Err(Error::AlreadyRunning);
    }
    Ok(lock)
}

pub async fn run(config: Config) -> crate::errors::Result<()> {
    if config.listen.iter().any(|a| !permitted_address(a.ip())) {
        return Err(Error::ListenerAddressNotPermitted);
    }
    let _lock = singleton(&config.directory)?;
    let core = Arc::new(Mutex::new(Core::open(&config.directory)?));
    let socket = config.directory.join(DEV_ENGINE_SOCKET);
    match fs::symlink_metadata(&socket) {
        Ok(m) if m.file_type().is_socket() => fs::remove_file(&socket)?,
        Ok(_) => return Err(Error::SocketPathNotSocket),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let unix = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let permits = Arc::new(Semaphore::new(128));
    let mut tasks = tokio::task::JoinSet::new();
    for address in config.listen {
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        update_pair_address(&core, address)?;
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
                        connection(core, false, false, reader, writer).await;
                    }
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), Error>(())
        });
    }
    {
        let core = Arc::clone(&core);
        let permits = Arc::clone(&permits);
        tasks.spawn(async move {
            loop {
                let (stream, _) = unix.accept().await?;
                let Some(peer_audit_token) = local_peer_audit_token(&stream) else {
                    continue;
                };
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    continue;
                };
                let core = Arc::clone(&core);
                tokio::spawn(async move {
                    let _permit = permit;
                    let signed_ui = tokio::task::spawn_blocking(move || {
                        ui_attestation::signed_ui_peer(peer_audit_token)
                    })
                    .await
                    .unwrap_or(false);
                    let (reader, writer) = transport::unix(stream);
                    connection(core, true, signed_ui, reader, writer).await;
                });
            }
            #[allow(unreachable_code)]
            Ok::<(), Error>(())
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
                    let mut core=core.lock().map_err(|_| crate::errors::internal("Engine state lock poisoned"))?;
                    let output=core.poll_output();let streams=core.pump_streams();
                    if prune==0 {core.store.prune().map_err(crate::errors::storage)?;}
                    output.and(streams)
                }).await?;
                prune=(prune+1)%6000;
                let error=result.err().map(|error| errors::kind(&error));
                if error!=previous_error {
                    if let Some(kind)=&error {eprintln!("{}",serde_json::json!({"event":"engine.storage_or_supervisor_error","kind":kind}));}
                    previous_error=error;
                }
            }
            #[allow(unreachable_code)] Ok::<(),Error>(())
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
        result=tasks.join_next()=>{if let Some(result)=result {result??;}},
    }
    tasks.abort_all();
    for client in lock_core(&core)?.clients.values() {
        client.outbox.close(CloseReason::ServerShutdown);
    }
    fs::remove_file(socket)?;
    Ok(())
}

async fn connection(
    core: Shared,
    local: bool,
    signed_ui: bool,
    mut reader: transport::Reader,
    writer: transport::Writer,
) {
    let (outbox, receiver) = outbox::channel();
    let id = match core
        .lock()
        .map_err(|_| Error::EngineStatePoisoned)
        .and_then(|mut state| state.connect(local, signed_ui, outbox.clone()))
    {
        Ok(id) => id,
        Err(_) => return,
    };
    let mut write_task = tokio::spawn(write_frames(receiver, writer));
    let reason = tokio::select! {
        reason=read_requests(Arc::clone(&core),&id,&outbox,&mut reader)=>reason,
        result=&mut write_task=>result.unwrap_or(CloseReason::WorkerFailed),
    };
    outbox.close(reason);
    write_task.abort();
    if let Ok(mut state) = core.lock() {
        let web_result = state.disconnect_web_host(&id);
        state.clients.remove(&id);
        state.native_waiters.retain(|_, waiters| {
            waiters.retain(|(connection, _)| connection != &id);
            !waiters.is_empty()
        });
        state.language_waiters.retain(|_, waiters| {
            waiters.retain(|(connection, _)| connection != &id);
            !waiters.is_empty()
        });
        if let Err(error) = web_result {
            eprintln!(
                "{}",
                serde_json::json!({
                    "event":"engine.storage_or_supervisor_error",
                    "kind":errors::kind(&error)
                })
            );
        }
    }
    eprintln!("{}", connection_close_record(&id, local, reason));
}

async fn write_frames(
    mut receiver: outbox::Receiver,
    mut writer: transport::Writer,
) -> CloseReason {
    let reason = loop {
        let mut packet = match receiver.next().await {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                break receiver
                    .close_reason()
                    .unwrap_or(CloseReason::WriterStopped);
            }
            Err(_) => break CloseReason::WorkerFailed,
        };
        if !packet.is_current() {
            continue;
        }
        if packet.closes_connection() {
            let reason = receiver.close_reason().unwrap_or(CloseReason::SlowConsumer);
            if matches!(
                tokio::time::timeout(Duration::from_secs(5), writer.write(&packet.frame)).await,
                Ok(Ok(()))
            ) {
                packet.written();
            }
            break reason;
        }
        let result = tokio::select! {
            result=writer.write(&packet.frame)=>result,
            reason=receiver.closed()=>{
                break reason;
            }
        };
        match result {
            Ok(()) => packet.written(),
            Err(_) => break CloseReason::WriteError,
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), writer.close()).await;
    reason
}

async fn next_frame(
    core: &Shared,
    id: &str,
    reader: &mut transport::Reader,
) -> Result<Frame, CloseReason> {
    let wait = {
        let core = core.lock().map_err(|_| CloseReason::WorkerFailed)?;
        let client = core.clients.get(id).ok_or(CloseReason::WorkerFailed)?;
        if client.actor.is_some() {
            None
        } else if client.pair_request.is_some() {
            Some(Duration::from_secs(600))
        } else {
            Some(Duration::from_secs(5))
        }
    };
    let result = if let Some(wait) = wait {
        tokio::time::timeout(wait, reader.next())
            .await
            .map_err(|_| CloseReason::HandshakeTimeout)?
    } else {
        reader.next().await
    };
    match result {
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(CloseReason::PeerClosed),
        Err(_) => Err(CloseReason::ReadError),
    }
}

async fn read_requests(
    core: Shared,
    id: &str,
    outbox: &Outbox,
    reader: &mut transport::Reader,
) -> CloseReason {
    loop {
        let frame = match next_frame(&core, id, reader).await {
            Ok(Frame::Json(bytes)) => bytes,
            Ok(Frame::Attachment { header, bytes }) => {
                let accepted = core
                    .lock()
                    .map_err(|_| Error::EngineStatePoisoned)
                    .and_then(|mut state| state.attachment_chunk(id, header, &bytes));
                if accepted.is_err() {
                    return CloseReason::InvalidFrame;
                }
                continue;
            }
            Ok(Frame::Terminal { .. }) => return CloseReason::InvalidFrame,
            Err(reason) => return reason,
        };
        let Ok(request) = serde_json::from_slice::<Value>(&frame) else {
            return CloseReason::InvalidJson;
        };
        if request.get("method").is_none() {
            let accepted = core
                .lock()
                .map_err(|_| Error::EngineStatePoisoned)
                .and_then(|mut state| state.web_host_response(id, request));
            if accepted.is_err() {
                return CloseReason::InvalidFrame;
            }
            continue;
        }
        let method = request["method"]
            .as_str()
            .and_then(|name| {
                soyeht_protocol::METHODS
                    .iter()
                    .find(|entry| entry.0 == name)
                    .map(|entry| entry.0)
            })
            .unwrap_or("unknown");
        let pane_id = operation_pane_id(method, &request).map(str::to_owned);
        let operation = OperationTimer::start(id, method, pane_id);
        let request_id = request["id"].clone();
        let failed_handshake = request["method"] == "hello";
        let shared = Arc::clone(&core);
        let connection = id.to_owned();
        let outcome = tokio::task::spawn_blocking(move || {
            shared
                .lock()
                .map_err(|_| Error::EngineStatePoisoned)?
                .dispatch(&connection, request)
        })
        .await;
        operation.finish(&outcome);
        match outcome {
            Ok(Ok(Outcome::Reply(result))) => {
                let Ok(frame) = model::response(request_id, result) else {
                    return CloseReason::WorkerFailed;
                };
                if !outbox.send(frame) {
                    return outbox.close_reason().unwrap_or(CloseReason::WriterStopped);
                }
            }
            Ok(Ok(Outcome::Attachment {
                result,
                reference,
                bytes,
            })) => {
                if let Err(reason) =
                    send_attachment_response(outbox, request_id, result, &reference, &bytes)
                {
                    return reason;
                }
            }
            Ok(Ok(Outcome::Queued)) => (),
            Ok(Ok(Outcome::Snapshot { parts, result })) => {
                for part in parts {
                    if outbox.write_page(part).await.is_err() {
                        return outbox
                            .close_reason()
                            .unwrap_or(CloseReason::SnapshotWriteFailed);
                    }
                }
                let Ok(frame) = model::response(request_id, result) else {
                    return CloseReason::WorkerFailed;
                };
                if !outbox.send(frame) {
                    return outbox.close_reason().unwrap_or(CloseReason::WriterStopped);
                }
            }
            Ok(Err(error)) if failed_handshake => {
                let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                    return CloseReason::WorkerFailed;
                };
                let _ = outbox.write_page(frame).await;
                return CloseReason::HandshakeRejected;
            }
            Ok(Err(error)) => {
                let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                    return CloseReason::WorkerFailed;
                };
                if !outbox.send(frame) {
                    return outbox.close_reason().unwrap_or(CloseReason::WriterStopped);
                }
            }
            Err(_) => return CloseReason::WorkerFailed,
        }
    }
}

fn send_attachment_response(
    outbox: &Outbox,
    request_id: Value,
    result: Value,
    reference: &AttachmentRef,
    bytes: &[u8],
) -> Result<(), CloseReason> {
    let frames = attachment_frames(reference, bytes).map_err(|_| CloseReason::WorkerFailed)?;
    for frame in frames {
        if !outbox.send_ordered(frame) {
            return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
        }
    }
    let frame = model::response(request_id, result).map_err(|_| CloseReason::WorkerFailed)?;
    if !outbox.send_ordered(frame) {
        return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
    }
    Ok(())
}

struct OperationTiming {
    started_at: chrono::DateTime<chrono::Utc>,
    finished_at: chrono::DateTime<chrono::Utc>,
    duration_ms: u128,
}

struct OperationTimer {
    operation_id: String,
    connection_id: String,
    method: &'static str,
    pane_id: Option<String>,
    started_at: chrono::DateTime<chrono::Utc>,
    started: Instant,
}

impl OperationTimer {
    fn start(connection: &str, method: &'static str, pane_id: Option<String>) -> Self {
        Self::start_with(
            connection,
            method,
            pane_id,
            chrono::Utc::now(),
            Instant::now(),
            emit_operation_record,
        )
    }

    fn start_with(
        connection: &str,
        method: &'static str,
        pane_id: Option<String>,
        started_at: chrono::DateTime<chrono::Utc>,
        started: Instant,
        emit: impl FnOnce(Value),
    ) -> Self {
        let timer = Self {
            operation_id: uuid::Uuid::new_v4().to_string(),
            connection_id: connection.to_owned(),
            method,
            pane_id,
            started_at,
            started,
        };
        emit(timer.started_record());
        timer
    }

    fn started_record(&self) -> Value {
        operation_started_record(
            &self.operation_id,
            &self.connection_id,
            self.method,
            self.pane_id.as_deref(),
            self.started_at,
        )
    }

    fn finish(
        self,
        outcome: &std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>,
    ) {
        let finished_at = chrono::Utc::now();
        let duration_ms = self.started.elapsed().as_millis();
        self.finish_with(outcome, finished_at, duration_ms, emit_operation_record);
    }

    fn finish_with(
        self,
        outcome: &std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>,
        finished_at: chrono::DateTime<chrono::Utc>,
        duration_ms: u128,
        emit: impl FnOnce(Value),
    ) {
        let record = operation_finished_record(
            &self.operation_id,
            &self.connection_id,
            self.method,
            self.pane_id.as_deref(),
            outcome,
            &OperationTiming {
                started_at: self.started_at,
                finished_at,
                duration_ms,
            },
        );
        emit(record);
    }
}

fn operation_finished_record(
    operation: &str,
    connection: &str,
    method: &str,
    pane_id: Option<&str>,
    outcome: &std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>,
    timing: &OperationTiming,
) -> Value {
    let (status, kind) = match outcome {
        Ok(Ok(Outcome::Reply(_))) => ("reply", None),
        Ok(Ok(Outcome::Attachment { .. })) => ("attachment", None),
        Ok(Ok(Outcome::Queued)) => ("queued", None),
        Ok(Ok(Outcome::Snapshot { .. })) => ("snapshot", None),
        Ok(Err(error)) => ("error", Some(errors::kind(error))),
        Err(_) => ("worker_failed", None),
    };
    operation_record(
        operation,
        connection,
        method,
        pane_id,
        status,
        kind.as_ref(),
        timing,
    )
}

fn emit_operation_record(record: Value) {
    eprintln!("{record}");
}

fn logged_method(method: &str) -> &'static str {
    soyeht_protocol::METHODS
        .iter()
        .find(|entry| entry.0 == method)
        .map(|entry| entry.0)
        .unwrap_or("unknown")
}

fn operation_started_record(
    operation: &str,
    connection: &str,
    method: &str,
    pane_id: Option<&str>,
    started_at: chrono::DateTime<chrono::Utc>,
) -> Value {
    serde_json::json!({
        "event": "engine.operation_started",
        "operationId": operation,
        "connectionId": connection,
        "method": logged_method(method),
        "paneId": pane_id,
        "startedAt": started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    })
}

fn connection_close_record(connection: &str, local: bool, reason: CloseReason) -> Value {
    serde_json::json!({
        "event": "engine.connection_closed",
        "connectionId": connection,
        "transport": if local { "local" } else { "websocket" },
        "reason": reason.as_str(),
    })
}

fn operation_record(
    operation: &str,
    connection: &str,
    method: &str,
    pane_id: Option<&str>,
    outcome: &str,
    kind: Option<&soyeht_protocol::ErrorKind>,
    timing: &OperationTiming,
) -> Value {
    // Only an allowlisted method name is retained. Request IDs and parameters
    // may contain credentials or terminal bytes and never enter this record.
    serde_json::json!({
        "event": "engine.operation",
        "operationId": operation,
        "connectionId": connection,
        "method": logged_method(method),
        "paneId": pane_id,
        "outcome": outcome,
        "kind": kind,
        "startedAt": timing.started_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "finishedAt": timing.finished_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "durationMs": timing.duration_ms,
    })
}

fn operation_pane_id<'a>(method: &str, request: &'a Value) -> Option<&'a str> {
    let route = soyeht_protocol::PANE_ROUTE_METHODS
        .iter()
        .find_map(|(candidate, property)| (*candidate == method).then_some(*property))?;
    let pane_id = request.get("params")?.get(route)?.as_str()?;
    let parsed = uuid::Uuid::parse_str(pane_id).ok()?;
    (parsed.hyphenated().to_string() == pane_id).then_some(pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(value: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .expect("valid test timestamp")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn operation_log_keeps_identity_but_never_unknown_method_text() {
        let operation = uuid::Uuid::new_v4().to_string();
        let connection = uuid::Uuid::new_v4().to_string();
        let timing = OperationTiming {
            started_at: timestamp("2026-09-17T10:00:00.123Z"),
            finished_at: timestamp("2026-09-17T10:00:00.135Z"),
            duration_ms: 12,
        };
        let record = operation_record(
            &operation,
            &connection,
            "untrusted-private-text",
            None,
            "error",
            Some(&soyeht_protocol::ErrorKind::InvalidParams),
            &timing,
        );
        assert_eq!(record["operationId"], operation);
        assert_eq!(record["connectionId"], connection);
        assert_eq!(record["method"], "unknown");
        assert_eq!(record["kind"], "invalid_params");
        assert_eq!(record["startedAt"], "2026-09-17T10:00:00.123Z");
        assert_eq!(record["finishedAt"], "2026-09-17T10:00:00.135Z");
        assert_eq!(record["durationMs"], 12);
        assert!(!record.to_string().contains("untrusted-private-text"));
        let started = operation_started_record(
            &operation,
            &connection,
            "untrusted-private-text",
            None,
            timing.started_at,
        );
        assert_eq!(started["event"], "engine.operation_started");
        assert_eq!(started["method"], "unknown");
        assert!(!started.to_string().contains("untrusted-private-text"));
        let success = operation_record(
            &operation,
            &connection,
            "pane.create",
            None,
            "reply",
            None,
            &timing,
        );
        assert_eq!(success["method"], "pane.create");
        assert!(success["kind"].is_null());
    }

    #[test]
    fn operation_start_is_durable_without_a_finish_and_correlates_when_finished() {
        let started_at = timestamp("2026-09-17T10:00:00.123Z");
        let mut open_records = Vec::new();
        let open = OperationTimer::start_with(
            "connection-open",
            "model.list",
            None,
            started_at,
            Instant::now(),
            |record| open_records.push(record),
        );
        let open_id = open.operation_id.clone();
        drop(open);

        assert_eq!(open_records.len(), 1);
        assert_eq!(open_records[0]["event"], "engine.operation_started");
        assert_eq!(open_records[0]["operationId"], open_id);
        assert_eq!(open_records[0]["connectionId"], "connection-open");
        assert_eq!(open_records[0]["method"], "model.list");
        assert_eq!(open_records[0]["startedAt"], "2026-09-17T10:00:00.123Z");
        assert!(open_records[0].get("finishedAt").is_none());
        assert!(open_records[0].get("durationMs").is_none());
        assert!(open_records[0].get("outcome").is_none());

        let mut completed_records = Vec::new();
        let completed = OperationTimer::start_with(
            "connection-finished",
            "harness.list",
            None,
            started_at,
            Instant::now(),
            |record| completed_records.push(record),
        );
        let completed_id = completed.operation_id.clone();
        let outcome: std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError> =
            Ok(Ok(Outcome::Reply(serde_json::json!({}))));
        completed.finish_with(
            &outcome,
            timestamp("2026-09-17T10:00:00.135Z"),
            12,
            |record| completed_records.push(record),
        );

        assert_eq!(completed_records.len(), 2);
        assert_eq!(completed_records[0]["event"], "engine.operation_started");
        assert_eq!(completed_records[1]["event"], "engine.operation");
        assert_eq!(completed_records[0]["operationId"], completed_id);
        assert_eq!(completed_records[1]["operationId"], completed_id);
        assert_eq!(completed_records[0]["connectionId"], "connection-finished");
        assert_eq!(completed_records[1]["connectionId"], "connection-finished");
        assert_eq!(completed_records[0]["method"], "harness.list");
        assert_eq!(completed_records[1]["method"], "harness.list");
        assert_eq!(
            completed_records[1]["finishedAt"],
            "2026-09-17T10:00:00.135Z"
        );
        assert_eq!(completed_records[1]["durationMs"], 12);
        assert_eq!(completed_records[1]["outcome"], "reply");
    }
    #[test]
    fn operation_log_adds_only_a_validated_pane_route() {
        let pane_id = uuid::Uuid::new_v4().to_string();
        let secret = "hook-capability-must-not-enter-the-log";
        let request = serde_json::json!({
            "method": "pane.stream.subscribe",
            "params": {"paneId":pane_id,"capability":secret},
        });
        let routed = operation_pane_id("pane.stream.subscribe", &request);
        let timing = OperationTiming {
            started_at: timestamp("2026-09-17T10:00:01.000Z"),
            finished_at: timestamp("2026-09-17T10:00:01.002Z"),
            duration_ms: 2,
        };
        let record = operation_record(
            "operation",
            "connection",
            "pane.stream.subscribe",
            routed,
            "queued",
            None,
            &timing,
        );
        assert_eq!(record["paneId"], pane_id);
        assert!(!record.to_string().contains(secret));

        let uppercase = serde_json::json!({
            "params": {"paneId":pane_id.to_uppercase(),"capability":secret},
        });
        assert!(operation_pane_id("pane.stream.subscribe", &uppercase).is_none());
        assert!(operation_pane_id("unknown", &request).is_none());

        let all_routes = serde_json::json!({
            "params": {"paneId":pane_id,"agentPaneId":pane_id},
        });
        let expected_methods = soyeht_protocol::PANE_ROUTE_METHODS
            .iter()
            .map(|(method, _)| *method)
            .collect::<Vec<_>>();
        let routed_methods = soyeht_protocol::METHODS
            .iter()
            .filter_map(|(method, _, _, _)| operation_pane_id(method, &all_routes).map(|_| *method))
            .collect::<Vec<_>>();
        assert_eq!(routed_methods, expected_methods);

        let writing = serde_json::json!({
            "params": {"agentPaneId":pane_id},
        });
        assert_eq!(
            operation_pane_id("writing.variant.request", &writing),
            Some(pane_id.as_str())
        );

        let usage = serde_json::json!({
            "method": "usage.refresh",
            "params": {"paneId":pane_id},
        });
        assert!(operation_pane_id("usage.refresh", &usage).is_none());
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

    #[test]
    fn connection_close_log_has_a_bounded_reason_and_no_transport_error_text() {
        let connection = uuid::Uuid::new_v4().to_string();
        let record = connection_close_record(&connection, true, CloseReason::WriteError);
        assert_eq!(record["event"], "engine.connection_closed");
        assert_eq!(record["connectionId"], connection);
        assert_eq!(record["transport"], "local");
        assert_eq!(record["reason"], "write_error");
        assert_eq!(record.as_object().unwrap().len(), 4);
    }
}
