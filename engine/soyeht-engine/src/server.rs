use crate::{
    admission::Admission,
    agents::catalog::prepare_catalog,
    core::Core,
    core_lock::{self, Shared},
    dispatch::Outcome,
    errors::{self, Error},
    hooks::Hooks,
    model, network,
    offload::Offload,
    operation_log::{OperationTimer, connection_close_record, logged_method, operation_pane_id},
    outbox::{self, CloseReason, Outbox},
    transport, ui_attestation,
};
use serde_json::Value;
use soyeht_platform::{
    fs::{ENGINE_LOCK, LockError, lock_singleton},
    local::{
        BindError,
        asynchronous::{Listener, Stream, same_user},
    },
    log,
};
use soyeht_protocol::{
    AttachmentRef,
    attachment::attachment_frames,
    frame::Frame,
    method,
    profile::{ENGINE_SOCKET, Profile},
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
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
};

/// Requests of one connection whose reply may overtake the ones behind it, in progress at once.
/// A connection with this many waits for one to finish before it reads the next request.
const CONCURRENT_OFFLOADS: usize = 8;

pub(crate) struct CatalogJob {
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

fn local_peer_audit_token(stream: &Stream) -> Option<ui_attestation::PeerAuditToken> {
    same_user(stream)
        .then(|| ui_attestation::peer_audit_token(stream))
        .flatten()
}

pub struct Config {
    pub directory: PathBuf,
    /// Which installed copy of Soyeht this engine is.
    pub profile: &'static Profile,
    pub listen: Vec<SocketAddr>,
    /// Also listen on every tailnet address on this port, following them as they come and go.
    pub tailnet_port: Option<u16>,
    /// The supervisor's program, when this engine starts the supervisor again after losing it
    /// (ADR 0108). `None` when something else keeps the supervisor alive.
    pub supervisor_program: Option<PathBuf>,
    /// What the engine does on a test's behalf; [`NoHooks`](crate::hooks::NoHooks) in a product.
    pub hooks: Arc<dyn Hooks>,
}

fn singleton(directory: &Path) -> crate::errors::Result<File> {
    match lock_singleton(directory, ENGINE_LOCK) {
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
    let mut core = Core::open_with(&config.directory, config.hooks)?;
    core.use_port_of(config.profile);
    if let Some(program) = config.supervisor_program {
        core.restart_supervisor_with(program);
    }
    let core = crate::push::share(core, &config.directory);
    let (catalog_tx, catalog_rx) = mpsc::channel(CATALOG_QUEUE_CAPACITY);
    tokio::spawn(catalog_worker(Arc::clone(&core), catalog_rx));
    let socket = config.directory.join(ENGINE_SOCKET);
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
        profile: config.profile,
        core: Arc::clone(&core),
        admission: Admission::default(),
        catalog_tx: catalog_tx.clone(),
    };
    for address in config.listen {
        let listener = TcpListener::bind(address).await?;
        network::update_pair_address(&core, listener.local_addr()?).await?;
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
pub(crate) struct Accept {
    pub(crate) profile: &'static Profile,
    pub(crate) core: Shared,
    pub(crate) admission: Admission,
    pub(crate) catalog_tx: mpsc::Sender<CatalogJob>,
}

/// Accepts the connections of this user on the local endpoint, for as long as the engine runs.
pub(crate) async fn serve_local(mut local: Listener, accept: Accept) -> crate::errors::Result<()> {
    let Accept {
        profile,
        core,
        admission,
        catalog_tx,
    } = accept;
    loop {
        let stream = local.accept().await?;
        let Some(peer_audit_token) = local_peer_audit_token(&stream) else {
            continue;
        };
        let Some(slot) = admission.local() else {
            continue;
        };
        let core = Arc::clone(&core);
        let catalog_tx = catalog_tx.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let signed_ui = tokio::task::spawn_blocking(move || {
                ui_attestation::signed_ui_peer(peer_audit_token, profile)
            })
            .await
            .unwrap_or(false);
            let (reader, writer) = transport::local(stream);
            connection(core, catalog_tx, true, signed_ui, reader, writer).await;
        });
    }
}

pub(crate) async fn serve_tcp(listener: TcpListener, accept: Accept) -> io::Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let Some(slot) = accept.admission.remote(peer.ip()) else {
            continue;
        };
        let (core, catalog_tx) = (Arc::clone(&accept.core), accept.catalog_tx.clone());
        tokio::spawn(async move {
            let _slot = slot;
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
        Err(error) => return log::failure("engine.connection_refused", &error),
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
        log::failure_with(
            "engine.disconnect_cleanup_failed",
            &[("connectionId", &id)],
            &error,
        );
    }
    connection_close_record(&id, local, reason).emit();
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
            Err(error) => {
                log::failure("engine.connection_writer_failed", &error);
                break CloseReason::WorkerFailed;
            }
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
            // ignored: the close reason is the record: it is logged when the connection closes.
            Err(_) => break CloseReason::WriteError,
        }
    };
    // ignored: bounded best effort; the reason below already says why the connection ended.
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
    let concurrent = Arc::new(Semaphore::new(CONCURRENT_OFFLOADS));
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
        let failed_handshake = request["method"] == method::HELLO;
        let connection = id.to_owned();
        let outcome = core_lock::with(&core, "dispatch", move |state| {
            state.dispatch(&connection, request)
        })
        .await;
        let outcome = match outcome {
            Ok(Outcome::Offload(offload)) if offload.is_concurrent() => {
                // Its reply goes out when the work is done. The requests behind it do not wait.
                let Ok(permit) = Arc::clone(&concurrent).acquire_owned().await else {
                    return CloseReason::WorkerFailed;
                };
                tokio::spawn(answer_concurrent(
                    Arc::clone(&core),
                    outbox.clone(),
                    Concurrent {
                        method,
                        request_id,
                        operation,
                        offload,
                    },
                    permit,
                ));
                continue;
            }
            other => resolve_offload(&core, other).await,
        };
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
        if let Err(reason) =
            send_outcome(outbox, method, request_id, failed_handshake, outcome).await
        {
            return reason;
        }
    }
}

/// A request whose work runs while the connection reads the next ones.
struct Concurrent {
    method: &'static str,
    request_id: Value,
    operation: OperationTimer,
    offload: Offload,
}

async fn answer_concurrent(
    core: Shared,
    outbox: Outbox,
    request: Concurrent,
    _permit: OwnedSemaphorePermit,
) {
    let outcome = resolve_offload(&core, Ok(Outcome::Offload(request.offload))).await;
    request.operation.finish(&outcome);
    if let Err(reason) =
        send_outcome(&outbox, request.method, request.request_id, false, outcome).await
    {
        outbox.close(reason);
    }
}

async fn send_outcome(
    outbox: &Outbox,
    method: &str,
    request_id: Value,
    failed_handshake: bool,
    outcome: crate::errors::Result<Outcome>,
) -> Result<(), CloseReason> {
    match outcome {
        Ok(Outcome::Reply(result)) => {
            let Ok(frame) = model::response(method, request_id, result) else {
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
            send_attachment_response(outbox, method, request_id, result, &reference, &bytes)?;
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
            let Ok(frame) = model::response(method, request_id, result) else {
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
            // ignored: best effort: the connection closes as rejected whether or not the error page lands.
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
                (Some(value), _) => {
                    model::response(&recipient.method, recipient.request_id, value.clone())
                }
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
            log::event(
                "engine.catalog_finalized",
                &[
                    ("method", &method),
                    (
                        "durationMs",
                        &finalize_started.elapsed().as_millis().to_string(),
                    ),
                ],
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
        Ok(Err(error)) => {
            log::event(
                "engine.offload_work_failed",
                &[(
                    "reason",
                    if error.is_panic() {
                        "panicked"
                    } else {
                        "cancelled"
                    },
                )],
            );
            expire
        }
        // ignored: the deadline passed: the offload's own expiry answers the request.
        Err(_) => expire,
    };
    core_lock::with(core, "offload_apply", apply).await
}

fn send_attachment_response(
    outbox: &Outbox,
    method: &str,
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
    let frame =
        model::response(method, request_id, result).map_err(|_| CloseReason::WorkerFailed)?;
    if !outbox.send_ordered(frame) {
        return Err(outbox.close_reason().unwrap_or(CloseReason::WriterStopped));
    }
    Ok(())
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
}
