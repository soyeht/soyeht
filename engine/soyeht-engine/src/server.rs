use crate::{
    agents::catalog::prepare_catalog,
    core::Core,
    dispatch::Outcome,
    errors::{self, Error},
    model, network,
    outbox::{self, CloseReason, Outbox},
    transport, ui_attestation,
};
use serde_json::Value;
use soyeht_protocol::{
    AttachmentRef, attachment::attachment_frames, frame::Frame, profile::DEV_ENGINE_SOCKET,
};
use std::{
    collections::{BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    io,
    net::SocketAddr,
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
    sync::{Semaphore, mpsc},
};

type Shared = Arc<Mutex<Core>>;

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
    /// Also listen on every tailnet address on this port, following them as they come and go.
    pub tailnet_port: Option<u16>,
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
    if config
        .listen
        .iter()
        .any(|a| !network::permitted_address(a.ip()))
    {
        return Err(Error::ListenerAddressNotPermitted);
    }
    let _lock = singleton(&config.directory)?;
    let core = crate::push::share(Core::open(&config.directory)?, &config.directory);
    let catalog_tx = start_workers(&core);
    let socket = config.directory.join(DEV_ENGINE_SOCKET);
    match fs::symlink_metadata(&socket) {
        Ok(m) if m.file_type().is_socket() => fs::remove_file(&socket)?,
        Ok(_) => return Err(Error::SocketPathNotSocket),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let unix = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let mut tasks = tokio::task::JoinSet::new();
    lock_core(&core)?.reachability = network::fixed_reachability(&config.listen);
    let accept = Accept {
        core: Arc::clone(&core),
        permits: Arc::new(Semaphore::new(128)),
        catalog_tx: catalog_tx.clone(),
    };
    for address in config.listen {
        let listener = TcpListener::bind(address).await?;
        update_pair_address(&core, listener.local_addr()?)?;
        let serving = serve_tcp(listener, accept.clone());
        tasks.spawn(async move { Ok::<(), Error>(serving.await?) });
    }
    if let Some(port) = config.tailnet_port {
        let serve = accept.clone();
        let serve = Box::new(move |listener| tokio::spawn(serve_tcp(listener, serve.clone())));
        let tailnet = network::TailnetListeners::system(port, serve);
        tasks.spawn(network::follow_tailnet(tailnet, Arc::clone(&core)).await?);
    }
    {
        let Accept {
            core,
            permits,
            catalog_tx,
        } = accept;
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
                let catalog_tx = catalog_tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let signed_ui = tokio::task::spawn_blocking(move || {
                        ui_attestation::signed_ui_peer(peer_audit_token)
                    })
                    .await
                    .unwrap_or(false);
                    let (reader, writer) = transport::unix(stream);
                    connection(core, catalog_tx, true, signed_ui, reader, writer).await;
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

/// What every accepted connection shares.
#[derive(Clone)]
struct Accept {
    core: Shared,
    permits: Arc<Semaphore>,
    catalog_tx: mpsc::Sender<CatalogJob>,
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

/// Starts the catalogue worker and the usage schedule; returns the catalogue queue.
fn start_workers(core: &Shared) -> mpsc::Sender<CatalogJob> {
    let (catalog_tx, catalog_rx) = mpsc::channel(CATALOG_QUEUE_CAPACITY);
    tokio::spawn(catalog_worker(Arc::clone(core), catalog_rx));
    tokio::spawn(crate::usage::schedule(Arc::clone(core)));
    catalog_tx
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
    let id = match core
        .lock()
        .map_err(|_| Error::EngineStatePoisoned)
        .and_then(|mut state| state.connect(local, signed_ui, outbox.clone()))
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
    if let Err(error) = lock_core(&core).and_then(|mut state| state.disconnect(&id)) {
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
    catalog_tx: &mpsc::Sender<CatalogJob>,
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
        let outcome = crate::deferred::resolve(Arc::clone(&core), outcome).await;
        let outcome = match outcome {
            Ok(Ok(Outcome::Catalog { method, params })) => {
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
    outcome: std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>,
) -> Result<(), CloseReason> {
    match outcome {
        Ok(Ok(Outcome::Reply(result))) => {
            let Ok(frame) = model::response(request_id, result) else {
                return Err(CloseReason::WorkerFailed);
            };
            if !outbox.send(frame) {
                return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
            }
        }
        Ok(Ok(Outcome::Attachment {
            result,
            reference,
            bytes,
        })) => {
            send_attachment_response(outbox, request_id, result, &reference, &bytes)?;
        }
        Ok(Ok(Outcome::Queued)) => (),
        Ok(Ok(Outcome::Snapshot { parts, result })) => {
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
        Ok(Ok(Outcome::Deferred(_) | Outcome::Catalog { .. })) => {
            return Err(CloseReason::WorkerFailed);
        }
        Ok(Err(error)) if failed_handshake => {
            let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                return Err(CloseReason::WorkerFailed);
            };
            // Best effort: the connection closes as rejected whether or not the error page lands.
            let _ = outbox.write_page(frame).await;
            return Err(CloseReason::HandshakeRejected);
        }
        Ok(Err(error)) => {
            let Ok(frame) = model::failure(request_id, errors::wire(error)) else {
                return Err(CloseReason::WorkerFailed);
            };
            if !outbox.send(frame) {
                return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
            }
        }
        Err(_) => return Err(CloseReason::WorkerFailed),
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
        let (directory, session_harnesses, epoch) = {
            let state = lock_core(&core)?;
            let ids: BTreeSet<_> = state
                .agents
                .values()
                .map(|agent| agent.state.launch.id.clone())
                .collect();
            (state.directory.clone(), ids, state.catalog_epoch)
        };
        let worker_method = method.to_owned();
        let worker_params = params.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            prepare_catalog(directory, &worker_method, &worker_params, session_harnesses)
        })
        .await
        .map_err(Error::Join)?;
        let state = lock_core(&core)?;
        if state.catalog_epoch == epoch {
            let finalize_started = Instant::now();
            let result = state.agent_catalog_prepared(prepared?, params);
            eprintln!(
                "{}",
                serde_json::json!({
                    "event":"engine.catalog_finalized",
                    "method":method,
                    "durationMs":finalize_started.elapsed().as_millis()
                })
            );
            return result;
        }
    }
    Err(errors::resync_required("Catalogue changed during refresh"))
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
    outcome: &std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError>,
) -> (&'static str, Option<soyeht_protocol::ErrorKind>) {
    match outcome {
        Ok(Ok(Outcome::Reply(_))) => ("reply", None),
        Ok(Ok(Outcome::Deferred(_))) => ("worker_failed", None),
        Ok(Ok(Outcome::Catalog { .. })) => ("worker_failed", None),
        Ok(Ok(Outcome::Attachment { .. })) => ("attachment", None),
        Ok(Ok(Outcome::Queued)) => ("queued", None),
        Ok(Ok(Outcome::Snapshot { .. })) => ("snapshot", None),
        Ok(Err(error)) => ("error", Some(errors::kind(error))),
        Err(_) => ("worker_failed", None),
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
        let core = Arc::new(Mutex::new(state));
        let (catalog_tx, mut catalog_rx) = mpsc::channel(CATALOG_QUEUE_CAPACITY);
        // Keep the receiver alive without running the catalogue worker.
        let (server, mut client) = UnixStream::pair().unwrap();
        let (mut reader, _) = transport::unix(server);
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
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn blocking_request_timeout_returns_an_error_instead_of_waiting_forever() {
        let result = crate::deferred::spawn_blocking_with_timeout(
            Duration::from_millis(5),
            || {
                std::thread::sleep(Duration::from_millis(50));
                Ok(())
            },
            "bounded work timed out",
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Err(Error::Internal(message)) if message == "bounded work timed out"
        ));
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
        let outcome: std::result::Result<crate::errors::Result<Outcome>, tokio::task::JoinError> =
            Ok(Ok(Outcome::Reply(serde_json::json!({}))));
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
