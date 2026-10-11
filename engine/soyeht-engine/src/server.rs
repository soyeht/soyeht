use crate::{
    agents::catalog::prepare_catalog,
    core::Core,
    core_lock::{self, Shared},
    dispatch::Outcome,
    errors::{self, Error},
    model, network,
    outbox::{self, CloseReason, Outbox},
    transport, ui_attestation,
};
use serde_json::Value;
use soyeht_platform::{
    fs::{LockError, lock_singleton},
    local::{
        BindError,
        asynchronous::{Listener, Stream, same_user},
    },
};
use soyeht_protocol::{
    AttachmentRef, attachment::attachment_frames, frame::Frame, profile::DEV_ENGINE_SOCKET,
};
use std::{
    collections::{BTreeSet, VecDeque},
    fs::{self, File},
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, mpsc},
};

struct CatalogJob {
    method: String,
    params: Value,
    request_id: Value,
    outbox: Outbox,
    operation: OperationTimer,
}

fn coalesce_catalog_jobs(
    active: CatalogJob,
    pending: &mut VecDeque<CatalogJob>,
) -> Vec<CatalogJob> {
    let mut recipients = vec![active];
    let mut remaining = VecDeque::new();
    while let Some(waiting) = pending.pop_front() {
        if waiting.method == recipients[0].method && waiting.params == recipients[0].params {
            recipients.push(waiting);
        } else {
            remaining.push_back(waiting);
        }
    }
    *pending = remaining;
    recipients
}

const CATALOG_QUEUE_CAPACITY: usize = 128;

async fn update_pair_address(shared: &Shared, address: SocketAddr) -> crate::errors::Result<()> {
    core_lock::with(shared, "pair_address", move |state| {
        if !address.ip().is_loopback() || state.pair_address.starts_with("127.") {
            state.pair_address = address.to_string();
        }
        Ok(())
    })
    .await
}

fn local_peer_audit_token(stream: &Stream) -> Option<ui_attestation::PeerAuditToken> {
    if !same_user(stream) {
        return None;
    }
    ui_attestation::peer_audit_token(stream)
}

pub struct Config {
    pub directory: PathBuf,
    pub listen: Vec<SocketAddr>,
    /// Also listen on every tailnet address on this port, following them as they come and go.
    pub tailnet_port: Option<u16>,
}

fn singleton(directory: &Path) -> crate::errors::Result<File> {
    match lock_singleton(directory, "engine.lock") {
        Ok(lock) => Ok(lock),
        Err(LockError::Held) => Err(Error::AlreadyRunning),
        Err(LockError::Io(error)) => Err(error.into()),
    }
}

pub async fn run(config: Config) -> crate::errors::Result<()> {
    if config
        .listen
        .iter()
        .any(|a| !network::permitted_address(a.ip()))
    {
        return Err(Error::ListenerAddressNotPermitted);
    }
    let _lock = singleton(&config.directory)?;
    let core = crate::push::share(Core::open(&config.directory)?, &config.directory);
    let (catalog_tx, catalog_rx) = mpsc::channel(CATALOG_QUEUE_CAPACITY);
    tokio::spawn(catalog_worker(Arc::clone(&core), catalog_rx));
    let socket = config.directory.join(DEV_ENGINE_SOCKET);
    let local = match Listener::bind(&socket) {
        Ok(listener) => listener,
        Err(BindError::PathNotEndpoint) => return Err(Error::SocketPathNotSocket),
        Err(BindError::Io(error)) => return Err(error.into()),
    };
    let mut tasks = tokio::task::JoinSet::new();
    let reachability = network::fixed_reachability(&config.listen);
    core_lock::with(&core, "reachability", move |state| {
        state.reachability = reachability;
        Ok(())
    })
    .await?;
    let accept = Accept {
        core: Arc::clone(&core),
        permits: Arc::new(Semaphore::new(128)),
        catalog_tx: catalog_tx.clone(),
    };
    for address in config.listen {
        let listener = TcpListener::bind(address).await?;
        update_pair_address(&core, listener.local_addr()?).await?;
        let serving = serve_tcp(listener, accept.clone());
        tasks.spawn(async move { Ok::<(), Error>(serving.await?) });
    }
    if let Some(port) = config.tailnet_port {
        let serve = accept.clone();
        let serve = Box::new(move |listener| tokio::spawn(serve_tcp(listener, serve.clone())));
        let tailnet = network::TailnetListeners::system(port, serve);
        tasks.spawn(network::follow_tailnet(tailnet, Arc::clone(&core)).await?);
    }
    tasks.spawn(serve_local(local, accept));
    tasks.spawn(crate::tick::run(Arc::clone(&core)));
    println!(
        "{}",
        serde_json::json!({"event":"engine.ready","socket":socket,"version":env!("CARGO_PKG_VERSION")})
    );
    let requested = soyeht_platform::shutdown::requested()?;
    tokio::select! {
        () = requested => (),
        result=tasks.join_next()=>{if let Some(result)=result {result??;}},
    }
    tasks.abort_all();
    core_lock::with(&core, "shutdown", |state| {
        for client in state.clients.values() {
            client.outbox.close(CloseReason::ServerShutdown);
        }
        Ok(())
    })
    .await?;
    fs::remove_file(socket)?;
    Ok(())
}

/// What every accepted connection shares.
#[derive(Clone)]
struct Accept {
    core: Shared,
    permits: Arc<Semaphore>,
    catalog_tx: mpsc::Sender<CatalogJob>,
}

/// Accepts the connections of this user on the local endpoint, for as long as the engine runs.
async fn serve_local(mut local: Listener, accept: Accept) -> crate::errors::Result<()> {
    let Accept {
        core,
        permits,
        catalog_tx,
    } = accept;
    loop {
        let stream = local.accept().await?;
        let Some(peer_audit_token) = local_peer_audit_token(&stream) else {
            continue;
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            continue;
        };
        let core = Arc::clone(&core);
        let catalog_tx = catalog_tx.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let signed_ui = tokio::task::spawn_blocking(move || {
                ui_attestation::signed_ui_peer(peer_audit_token)
            })
            .await
            .unwrap_or(false);
            let (reader, writer) = transport::local(stream);
            connection(core, catalog_tx, true, signed_ui, reader, writer).await;
        });
    }
}

async fn serve_tcp(listener: TcpListener, accept: Accept) -> io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = Arc::clone(&accept.permits).try_acquire_owned() else {
            continue;
        };
        let (core, catalog_tx) = (Arc::clone(&accept.core), accept.catalog_tx.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let websocket = transport::websocket(stream);
            if let Ok(Ok((reader, writer))) =
                tokio::time::timeout(Duration::from_secs(5), websocket).await
            {
                connection(core, catalog_tx, false, false, reader, writer).await;
            }
        });
    }
}

async fn connection(
    core: Shared,
    catalog_tx: mpsc::Sender<CatalogJob>,
    local: bool,
    signed_ui: bool,
    mut reader: transport::Reader,
    writer: transport::Writer,
) {
    let (outbox, receiver) = outbox::channel();
    let connected = outbox.clone();
    let id = match core_lock::with(&core, "connect", move |state| {
        state.connect(local, signed_ui, connected)
    })
    .await
    {
        Ok(id) => id,
        Err(error) => return errors::log_failure("engine.connection_refused", &error),
    };
    let mut write_task = tokio::spawn(write_frames(receiver, writer));
    let reason = tokio::select! {
        reason=read_requests(Arc::clone(&core),&catalog_tx,&id,&outbox,&mut reader)=>reason,
        result=&mut write_task=>result.unwrap_or(CloseReason::WorkerFailed),
    };
    outbox.close(reason);
    write_task.abort();
    let departed = id.clone();
    if let Err(error) = core_lock::with(&core, "disconnect", move |state| {
        state.disconnect(&departed)
    })
    .await
    {
        // A poisoned lock leaves the client and its waiters registered: say why.
        let kind = errors::kind(&error);
        let record = serde_json::json!({"event":"engine.disconnect_cleanup_failed","connectionId":id,"kind":kind});
        eprintln!("{record}");
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
    // Bounded best effort; the reason below already says why the connection ended.
    let _ = tokio::time::timeout(Duration::from_secs(1), writer.close()).await;
    reason
}

async fn read_requests(
    core: Shared,
    catalog_tx: &mpsc::Sender<CatalogJob>,
    id: &str,
    outbox: &Outbox,
    reader: &mut transport::Reader,
) -> CloseReason {
    let mut authenticated = false;
    loop {
        let frame = match crate::frames::next_frame(&core, id, reader, &mut authenticated).await {
            Ok(Frame::Json(bytes)) => bytes,
            Ok(Frame::Attachment { header, bytes }) => {
                let connection = id.to_owned();
                let accepted = core_lock::with(&core, "attachment", move |state| {
                    state.attachment_chunk(&connection, header, &bytes)
                })
                .await;
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
            let connection = id.to_owned();
            let accepted = core_lock::with(&core, "web_host_response", move |state| {
                state.web_host_response(&connection, request)
            })
            .await;
            if accepted.is_err() {
                return CloseReason::InvalidFrame;
            }
            continue;
        }
        let method = request["method"].as_str().map_or("unknown", logged_method);
        let pane_id = operation_pane_id(method, &request).map(str::to_owned);
        let operation = OperationTimer::start(id, method, pane_id);
        let request_id = request["id"].clone();
        let failed_handshake = request["method"] == "hello";
        let connection = id.to_owned();
        let outcome = core_lock::with(&core, "dispatch", move |state| {
            state.dispatch(&connection, request)
        })
        .await;
        let outcome = resolve_offload(&core, outcome).await;
        let outcome = match outcome {
            Ok(Outcome::Catalog { method, params }) => {
                let job = CatalogJob {
                    method,
                    params,
                    request_id,
                    outbox: outbox.clone(),
                    operation,
                };
                if let Err(error) = catalog_tx.try_send(job) {
                    match error {
                        mpsc::error::TrySendError::Full(job) => {
                            let failure = errors::slow_consumer("Catalogue queue is full; retry");
                            job.operation
                                .finish_status("error", Some(&errors::kind(&failure)));
                            let Ok(frame) = model::failure(job.request_id, errors::wire(failure))
                            else {
                                return CloseReason::WorkerFailed;
                            };
                            if !outbox.send(frame) {
                                return outbox.close_reason().unwrap_or(CloseReason::WriterStopped);
                            }
                        }
                        mpsc::error::TrySendError::Closed(job) => {
                            job.operation.finish_status("worker_failed", None);
                            return CloseReason::WorkerFailed;
                        }
                    }
                }
                continue;
            }
            other => other,
        };
        operation.finish(&outcome);
        if let Err(reason) = send_outcome(outbox, request_id, failed_handshake, outcome).await {
            return reason;
        }
    }
}

async fn send_outcome(
    outbox: &Outbox,
    request_id: Value,
    failed_handshake: bool,
    outcome: crate::errors::Result<Outcome>,
) -> Result<(), CloseReason> {
    match outcome {
        Ok(Outcome::Reply(result)) => {
            let Ok(frame) = model::response(request_id, result) else {
                return Err(CloseReason::WorkerFailed);
            };
            if !outbox.send(frame) {
                return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
            }
        }
        Ok(Outcome::Attachment {
            result,
            reference,
            bytes,
        }) => {
            send_attachment_response(outbox, request_id, result, &reference, &bytes)?;
        }
        Ok(Outcome::Queued) => (),
        Ok(Outcome::Snapshot { parts, result }) => {
            for part in parts {
                if outbox.write_page(part).await.is_err() {
                    return Err(outbox
                        .close_reason()
                        .unwrap_or(CloseReason::SnapshotWriteFailed));
                }
            }
            let Ok(frame) = model::response(request_id, result) else {
                return Err(CloseReason::WorkerFailed);
            };
            if !outbox.send(frame) {
                return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
            }
        }
        Ok(Outcome::Offload(_)) | Ok(Outcome::Catalog { .. }) | Err(Error::Join(_)) => {
            return Err(CloseReason::WorkerFailed);
        }
        Err(error) if failed_handshake => {
            let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                return Err(CloseReason::WorkerFailed);
            };
            // Best effort: the connection closes as rejected whether or not the error page lands.
            let _ = outbox.write_page(frame).await;
            return Err(CloseReason::HandshakeRejected);
        }
        Err(error) => {
            let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                return Err(CloseReason::WorkerFailed);
            };
            if !outbox.send(frame) {
                return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
            }
        }
    }
    Ok(())
}

/// Catalog requests share one worker across all connections. Identical requests
/// waiting behind the active scan receive its result instead of starting more scans.
async fn catalog_worker(core: Shared, mut rx: mpsc::Receiver<CatalogJob>) {
    let mut pending = VecDeque::new();
    loop {
        let job = match pending.pop_front() {
            Some(job) => job,
            None => match rx.recv().await {
                Some(job) => job,
                None => break,
            },
        };
        let result = catalog_result(Arc::clone(&core), &job.method, &job.params).await;
        while let Ok(waiting) = rx.try_recv() {
            pending.push_back(waiting);
        }
        let recipients = coalesce_catalog_jobs(job, &mut pending);
        let (status, kind) = match &result {
            Ok(_) => ("reply", None),
            Err(error) => ("error", Some(errors::kind(error))),
        };
        let (value, wire_error) = match result {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(errors::wire(error))),
        };
        for recipient in recipients {
            recipient.operation.finish_status(status, kind.as_ref());
            if recipient.outbox.is_closed() {
                continue;
            }
            let frame = match (&value, &wire_error) {
                (Some(value), _) => model::response(recipient.request_id, value.clone()),
                (_, Some(error)) => model::failure(recipient.request_id, error.clone()),
                _ => continue,
            };
            if let Ok(frame) = frame {
                // A refused send already closed that outbox with its own reason.
                recipient.outbox.send(frame);
            }
        }
    }
}

async fn catalog_result(
    core: Shared,
    method: &str,
    params: &Value,
) -> crate::errors::Result<Value> {
    // A custom-harness mutation may land while the filesystem scan runs. Retry
    // against the newer catalogue rather than publishing that stale snapshot.
    for _ in 0..3 {
        let (directory, session_harnesses, epoch) =
            core_lock::with(&core, "catalog_snapshot", |state| {
                let ids: BTreeSet<_> = state
                    .agents
                    .values()
                    .map(|agent| agent.state.launch.id.clone())
                    .collect();
                Ok((state.directory.clone(), ids, state.catalog_epoch))
            })
            .await?;
        let worker_method = method.to_owned();
        let worker_params = params.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_catalog(directory, &worker_method, &worker_params, session_harnesses)
        })
        .await
        .map_err(Error::Join)?;
        let (method, params) = (method.to_owned(), params.clone());
        let finalized = core_lock::with(&core, "catalog_finalize", move |state| {
            if state.catalog_epoch != epoch {
                return Ok(None);
            }
            let finalize_started = Instant::now();
            let result = state.agent_catalog_prepared(prepared?, &params);
            eprintln!(
                "{}",
                serde_json::json!({
                    "event":"engine.catalog_finalized",
                    "method":method,
                    "durationMs":finalize_started.elapsed().as_millis()
                })
            );
            result.map(Some)
        })
        .await?;
        if let Some(result) = finalized {
            return Ok(result);
        }
    }
    Err(errors::resync_required("Catalogue changed during refresh"))
}

/// Runs an offloaded request's work on a blocking thread with no lock, then
/// applies its result under the Core lock (ADR 0079). Late or failed work is
/// answered with the request's timeout error; its result is discarded.
async fn resolve_offload(
    core: &Shared,
    outcome: crate::errors::Result<Outcome>,
) -> crate::errors::Result<Outcome> {
    let Ok(Outcome::Offload(offload)) = outcome else {
        return outcome;
    };
    let (timeout, work, expire) = offload.into_parts();
    let apply = match tokio::time::timeout(timeout, tokio::task::spawn_blocking(work)).await {
        Ok(Ok(apply)) => apply,
        Ok(Err(_)) | Err(_) => expire,
    };
    core_lock::with(core, "offload_apply", apply).await
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

    fn finish(self, outcome: &crate::errors::Result<Outcome>) {
        let (status, kind) = operation_status(outcome);
        self.finish_status(status, kind.as_ref());
    }

    fn finish_status(self, status: &str, kind: Option<&soyeht_protocol::ErrorKind>) {
        let finished_at = chrono::Utc::now();
        let duration_ms = self.started.elapsed().as_millis();
        self.finish_with(
            status,
            kind,
            finished_at,
            duration_ms,
            emit_operation_record,
        );
    }

    fn finish_with(
        self,
        status: &str,
        kind: Option<&soyeht_protocol::ErrorKind>,
        finished_at: chrono::DateTime<chrono::Utc>,
        duration_ms: u128,
        emit: impl FnOnce(Value),
    ) {
        emit(operation_record(
            &self.operation_id,
            &self.connection_id,
            self.method,
            self.pane_id.as_deref(),
            status,
            kind,
            &OperationTiming {
                started_at: self.started_at,
                finished_at,
                duration_ms,
            },
        ));
    }
}

fn operation_status(
    outcome: &crate::errors::Result<Outcome>,
) -> (&'static str, Option<soyeht_protocol::ErrorKind>) {
    match outcome {
        Ok(Outcome::Reply(_)) => ("reply", None),
        Ok(Outcome::Offload(_) | Outcome::Catalog { .. }) | Err(Error::Join(_)) => {
            ("worker_failed", None)
        }
        Ok(Outcome::Attachment { .. }) => ("attachment", None),
        Ok(Outcome::Queued) => ("queued", None),
        Ok(Outcome::Snapshot { .. }) => ("snapshot", None),
        Err(error) => ("error", Some(errors::kind(error))),
    }
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
    use tokio::io::AsyncWriteExt;

    #[test]
    fn repeated_catalog_requests_share_the_active_scan_but_distinct_requests_wait() {
        fn job(method: &str, params: Value, request_id: u64) -> CatalogJob {
            CatalogJob {
                method: method.into(),
                params,
                request_id: serde_json::json!(request_id),
                outbox: outbox::channel().0,
                operation: OperationTimer::start_with(
                    "test",
                    logged_method(method),
                    None,
                    chrono::Utc::now(),
                    Instant::now(),
                    |_| {},
                ),
            }
        }
        let scan = serde_json::json!({"rescan":true,"limit":64});
        let active = job("harness.list", scan.clone(), 1);
        let mut pending = VecDeque::from([
            job("harness.list", scan.clone(), 2),
            job("model.list", serde_json::json!({"harness":"other"}), 3),
            job("harness.list", scan, 4),
        ]);
        let joined = coalesce_catalog_jobs(active, &mut pending);
        assert_eq!(
            joined.iter().map(|job| &job.request_id).collect::<Vec<_>>(),
            [
                &serde_json::json!(1),
                &serde_json::json!(2),
                &serde_json::json!(4),
            ]
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, 3);
    }

    #[tokio::test]
    async fn a_blocked_catalog_worker_does_not_hold_the_next_rpc_on_the_same_socket() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-catalog-{}", uuid::Uuid::new_v4()));
        let mut state = Core::open(&directory).unwrap();
        let (outbox, mut replies) = outbox::channel();
        let connection = state.connect(true, false, outbox.clone()).unwrap();
        state
            .dispatch(
                &connection,
                serde_json::json!({
                    "jsonrpc":"2.0", "id":0, "method":"hello",
                    "params":{"client":"catalog-test","clientVersion":"1","protocolVersion":1}
                }),
            )
            .unwrap();
        let core = Arc::new(std::sync::Mutex::new(state));
        let (catalog_tx, mut catalog_rx) = mpsc::channel(CATALOG_QUEUE_CAPACITY);
        // Keep the receiver alive without running the catalogue worker.
        let (server, mut client) = soyeht_platform::local::asynchronous::pair().unwrap();
        let (mut reader, _) = transport::local(server);
        let task = tokio::spawn({
            let core = Arc::clone(&core);
            let connection = connection.clone();
            async move { read_requests(core, &catalog_tx, &connection, &outbox, &mut reader).await }
        });
        for (id, method) in [(1, "harness.list"), (2, "engine.status")] {
            let mut bytes = Vec::new();
            Frame::Json(
                serde_json::to_vec(&serde_json::json!({
                    "jsonrpc":"2.0", "id":id, "method":method, "params":{}
                }))
                .unwrap(),
            )
            .write_to(&mut bytes)
            .unwrap();
            client.write_all(&bytes).await.unwrap();
        }
        let queued = tokio::time::timeout(Duration::from_secs(1), catalog_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(queued.method, "harness.list");
        let response = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let packet = replies.next().await.unwrap().unwrap();
                if let Frame::Json(bytes) = packet.frame {
                    let value: Value = serde_json::from_slice(&bytes).unwrap();
                    if value["id"] == 2 {
                        break value;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert!(response["result"]["generation"].is_number());
        task.abort();
        // The aborted task still owns a handle to the core until it has been dropped.
        let _ = task.await;
        crate::test_paths::remove_after(core, directory);
    }

    #[tokio::test]
    async fn offloaded_request_timeout_returns_an_error_instead_of_waiting_forever() {
        let directory =
            std::env::temp_dir().join(format!("soyeht-offload-timeout-{}", uuid::Uuid::new_v4()));
        let core = Arc::new(std::sync::Mutex::new(Core::open(&directory).unwrap()));
        let offload = crate::offload::Offload::new(
            "bounded work timed out",
            Duration::from_millis(5),
            || {
                std::thread::sleep(Duration::from_millis(50));
                Ok(())
            },
            |_, result| result.map(|()| Outcome::Queued),
        );
        let result = resolve_offload(&core, Ok(Outcome::Offload(offload))).await;
        assert!(matches!(
            result,
            Err(Error::Internal(message)) if message == "bounded work timed out"
        ));
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

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
        let outcome: crate::errors::Result<Outcome> = Ok(Outcome::Reply(serde_json::json!({})));
        let (status, kind) = operation_status(&outcome);
        completed.finish_with(
            status,
            kind.as_ref(),
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
            "params": {"paneId":pane_id,"agentPaneId":pane_id,"targetPaneId":pane_id},
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
