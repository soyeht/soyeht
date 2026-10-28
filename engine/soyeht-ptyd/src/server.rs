use crate::{
    Error, Result,
    child::{Child, Reach},
    io::Output,
    pending::Pending,
    pipe_guard::Guard,
    process,
    pty::Resizer,
    registry::Registry,
    retention::Retention,
    wire::*,
    wire_kind,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_platform::{
    fs::{LockError, lock_singleton},
    local::{BindError, Listener, Stream},
    log,
};
use soyeht_protocol::{
    frame::{read_json, write_json},
    limits,
    profile::PTYD_SOCKET,
};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

struct Live {
    master: Mutex<File>,
    resizer: Option<Resizer>,
    child: Mutex<Child>,
    output: Mutex<OutputState>,
    guard: Option<Mutex<Guard>>,
    exited: AtomicBool,
    read_done: AtomicBool,
}

struct OutputState {
    retention: Retention,
    pending: Option<Pending>,
}

struct State {
    directory: PathBuf,
    supervisor_id: String,
    registry: Registry,
    live: BTreeMap<String, Arc<Live>>,
    active: Option<(String, Stream)>,
    inputs: BTreeMap<String, (String, String, usize)>,
}

fn error(kind: &str, message: &str) -> Response {
    Response::Error {
        kind: kind.into(),
        message: message.into(),
    }
}

fn lock<'a, T>(mutex: &'a Mutex<T>, resource: &'static str) -> Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| Error::PoisonedLock(resource))
}

fn send(stream: &mut Stream, response: &Response) -> Result<()> {
    write_json(stream, response)?;
    Ok(())
}

fn refuse(stream: &mut Stream, kind: &str, message: &str) -> Result<()> {
    send(stream, &error(kind, message))
}

pub fn run(directory: &Path) -> Result<()> {
    let _lock = match lock_singleton(directory, "ptyd.lock") {
        Ok(lock) => lock,
        Err(LockError::Held) => return Err(Error::AlreadyRunning),
        Err(LockError::Io(error)) => return Err(error.into()),
    };
    let endpoint = directory.join(PTYD_SOCKET);
    let listener = match Listener::bind(&endpoint) {
        Ok(listener) => listener,
        Err(BindError::PathNotEndpoint) => return Err(Error::SocketPathNotSocket),
        Err(BindError::Io(error)) => return Err(error.into()),
    };
    let _endpoint = EndpointFile(endpoint);
    let registry = Registry::load(directory)?;
    let state = Arc::new(Mutex::new(State {
        directory: directory.to_owned(),
        supervisor_id: Uuid::new_v4().to_string(),
        registry,
        live: BTreeMap::new(),
        active: None,
        inputs: BTreeMap::new(),
    }));
    println!(
        "{}",
        log::record("ptyd.ready", &[("version", &VERSION.to_string())])
    );
    for incoming in listener.incoming() {
        let stream = incoming?;
        let state = Arc::clone(&state);
        thread::spawn(move || {
            if let Err(error) = connection(stream, state) {
                log::event("ptyd.connection_failed", &[("error", &error_label(&error))]);
            }
        });
    }
    Ok(())
}

/// The endpoint this supervisor listens on; it is removed when the supervisor stops, as the
/// engine removes its own. A supervisor that is killed cannot run this, and the next one
/// replaces the file it left.
struct EndpointFile(PathBuf);

impl Drop for EndpointFile {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => log::failure("ptyd.endpoint_not_removed", &error),
        }
    }
}

/// A bounded label for logs: the variant (and I/O kind), never message text or paths.
fn error_label(error: &Error) -> String {
    match error {
        Error::Io(error) => format!("Io::{:?}", error.kind()),
        other => format!("{other:?}")
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or("Unknown")
            .to_string(),
    }
}

fn connection(mut stream: Stream, shared: Arc<Mutex<State>>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let Some(request) = read_json::<Request>(&mut stream)? else {
        return Ok(());
    };
    let Request::Hello {
        version,
        engine_generation,
    } = request
    else {
        return refuse(&mut stream, wire_kind::UNAUTHORIZED, "hello required");
    };
    let connection_id = Uuid::new_v4().to_string();
    {
        let mut state = lock(&shared, "state")?;
        if version != VERSION {
            return refuse(
                &mut stream,
                wire_kind::VERSION_UNSUPPORTED,
                "incompatible supervisor protocol",
            );
        }
        if engine_generation < state.registry.engine_generation {
            return refuse(
                &mut stream,
                wire_kind::STALE_ENGINE_GENERATION,
                "engine generation is stale",
            );
        }
        if engine_generation == state.registry.engine_generation && state.active.is_some() {
            return refuse(
                &mut stream,
                wire_kind::ENGINE_CONNECTED,
                "an engine is already connected",
            );
        }
        let prior_generation = state.registry.engine_generation;
        state.registry.engine_generation = engine_generation;
        if let Err(error) = state.registry.save(&state.directory) {
            state.registry.engine_generation = prior_generation;
            return Err(error);
        }
        if let Some((_, old)) = state.active.take() {
            // The superseded engine connection may already be closed; either way it ends here.
            let _ = old.shutdown(Shutdown::Both);
        }
        state.active = Some((connection_id.clone(), stream.try_clone()?));
        if let Err(error) = send(
            &mut stream,
            &Response::Welcome {
                version: VERSION,
                supervisor_id: state.supervisor_id.clone(),
                supervisor_generation: state.registry.supervisor_generation,
            },
        ) {
            state.active = None;
            return Err(error);
        }
    }
    let result: Result<()> = (|| {
        stream.set_read_timeout(None)?;
        while let Some(request) = read_json(&mut stream)? {
            let mut state = lock(&shared, "state")?;
            if state.active.as_ref().map(|(id, _)| id.as_str()) != Some(connection_id.as_str()) {
                return Err(Error::ConnectionSuperseded);
            }
            let response = execute(&mut state, &shared, request)
                .unwrap_or_else(|e| error(wire_kind::OPERATION_FAILED, &e.to_string()));
            drop(state);
            send(&mut stream, &response)?;
        }
        Ok(())
    })();
    let mut state = lock(&shared, "state")?;
    if state.active.as_ref().map(|(id, _)| id.as_str()) == Some(connection_id.as_str()) {
        state.active = None;
    }
    result
}

fn execute(state: &mut State, shared: &Arc<Mutex<State>>, request: Request) -> Result<Response> {
    match request {
        Request::Hello { .. } => execute_hello(),
        Request::List => execute_list(state),
        Request::Spawn(config) => execute_spawn(state, shared, config),
        Request::Kill { instance_id } => execute_kill(state, &instance_id),
        Request::Close { instance_id } => execute_close(state, &instance_id),
        Request::PendingInputs => execute_pending_inputs(state),
        Request::PendingRequests { instance_id } => execute_pending_requests(state, &instance_id),
        Request::PendingRequestChunk {
            instance_id,
            request_key,
            offset,
        } => execute_pending_request_chunk(state, &instance_id, request_key, offset),
        Request::ConfirmInput { operation_key } => execute_confirm_input(state, &operation_key),
        Request::Input {
            instance_id,
            bytes,
            operation_key,
            fingerprint,
        } => execute_input(state, instance_id, bytes, operation_key, fingerprint),
        Request::Resize {
            instance_id,
            cols,
            rows,
        } => execute_resize(state, &instance_id, cols, rows),
        Request::Drain {
            instance_id,
            from_seq,
        } => execute_drain(state, &instance_id, from_seq),
        Request::Ack { instance_id, seq } => execute_ack(state, &instance_id, seq),
    }
}

fn execute_hello() -> Result<Response> {
    Ok(error(
        wire_kind::ALREADY_AUTHENTICATED,
        "hello was already accepted",
    ))
}

fn execute_list(state: &State) -> Result<Response> {
    Ok(Response::Instances(
        state
            .registry
            .instances
            .values()
            .filter(|instance| !instance.closed)
            .cloned()
            .collect(),
    ))
}

fn execute_spawn(state: &mut State, shared: &Arc<Mutex<State>>, config: Spawn) -> Result<Response> {
    if let Some(existing) = state.registry.instances.get(&config.instance_key) {
        return Ok(if existing.spawn == config {
            Response::Instance(existing.clone())
        } else {
            error(
                wire_kind::IDEMPOTENCY_CONFLICT,
                "spawn key has different parameters",
            )
        });
    }
    let instance_id = Uuid::new_v4().to_string();
    let process::Spawned {
        child,
        input,
        output: reader,
        pid,
        resizer,
        guard,
    } = match process::spawn(&config, &state.directory, &instance_id) {
        Ok(spawned) => spawned,
        Err(failure) if failure.spawn_refused() => {
            // Setup either never starts a child or kills and reaps it before returning an
            // error. No process needs adoption.
            return Ok(error(wire_kind::SPAWN_REFUSED, &failure.to_string()));
        }
        Err(failure) if config.kind == ProcessKind::Pty => {
            // Terminal setup also leaves no process behind, whatever the cause.
            return Ok(error(wire_kind::SPAWN_FAILED, &failure.to_string()));
        }
        Err(failure) => return Err(failure),
    };
    let pipes = config.kind == ProcessKind::PipesAcp;
    let instance = Instance {
        instance_id,
        supervisor_id: state.supervisor_id.clone(),
        pid,
        spawn: config,
        closed: false,
        exit_code: None,
    };
    let live = Arc::new(Live {
        master: Mutex::new(input),
        resizer,
        child: Mutex::new(child),
        output: Mutex::new(OutputState {
            retention: Retention::new(limits::PTY_RETENTION_BYTES)?,
            pending: pipes.then(Pending::default),
        }),
        guard: guard.map(Mutex::new),
        exited: AtomicBool::new(false),
        read_done: AtomicBool::new(false),
    });
    state
        .registry
        .instances
        .insert(instance.spawn.instance_key.clone(), instance.clone());
    if let Err(failure) = state.registry.save(&state.directory) {
        state
            .registry
            .instances
            .remove(&instance.spawn.instance_key);
        let mut child = lock(&live.child, "child")?;
        terminate(&live, &child);
        reap(&mut child);
        return Err(failure);
    }
    state
        .live
        .insert(instance.instance_id.clone(), Arc::clone(&live));
    start_readers(
        reader,
        live,
        Arc::clone(shared),
        instance.spawn.instance_key.clone(),
    );
    Ok(Response::Instance(instance))
}

/// Ends an instance's process. A guarded process is an agent adapter or a
/// language server, whose whole process tree must go with it.
fn terminate(live: &Live, child: &Child) {
    child.kill(if live.guard.is_some() {
        Reach::Tree
    } else {
        Reach::Group
    });
}

/// How long a kill waits for the child to be gone while holding its lock.
const REAP_LIMIT: Duration = Duration::from_secs(2);

/// Collects the exit of a child that was just terminated. The wait is bounded because the
/// caller holds the child's lock, which the exit watcher and every other request need; a child
/// that outlasts the limit is collected by the watcher when it does end.
fn reap(child: &mut Child) {
    match child.wait_within(REAP_LIMIT) {
        Ok(Some(_)) => {}
        Ok(None) => log::event("ptyd.child_still_running_after_kill", &[]),
        // The child could not be queried; the watcher reports it.
        Err(error) => log::failure("ptyd.child_wait_failed", &error),
    }
}

fn execute_kill(state: &State, instance_id: &str) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    let mut child = lock(&live.child, "child")?;
    if child.try_wait()?.is_none() {
        terminate(live, &child);
        reap(&mut child);
    }
    // Keep the live output and registry entry until Close. ACP kill stops a
    // command but leaves its output available to the caller.
    Ok(Response::Ok)
}

fn execute_close(state: &mut State, instance_id: &str) -> Result<Response> {
    if let Some(instance) = state
        .registry
        .instances
        .values_mut()
        .find(|instance| instance.instance_id == instance_id)
    {
        let key = instance.spawn.instance_key.clone();
        let was_closed = instance.closed;
        instance.closed = true;
        if !was_closed && let Err(failure) = state.registry.save(&state.directory) {
            let restored = state
                .registry
                .instances
                .get_mut(&key)
                .ok_or(Error::RegistryInstanceMissing)?;
            restored.closed = false;
            return Err(failure);
        }
        if let Some(live) = state.live.remove(instance_id) {
            let mut child = lock(&live.child, "child")?;
            if child.try_wait()?.is_none() {
                terminate(&live, &child);
                reap(&mut child);
            }
            live.exited.store(true, Ordering::Release);
        }
    }
    Ok(Response::Ok)
}

fn execute_pending_inputs(state: &State) -> Result<Response> {
    Ok(Response::InputReceipts(
        state.inputs.keys().cloned().collect(),
    ))
}

fn execute_pending_requests(state: &State, instance_id: &str) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    let output = lock(&live.output, "output")?;
    let Some(pending) = &output.pending else {
        return Ok(error(wire_kind::WRONG_PROCESS_KIND, "ACP pipes required"));
    };
    let (items, recovery_gap) = pending.list();
    Ok(Response::PendingRequests {
        items,
        recovery_gap,
    })
}

fn execute_pending_request_chunk(
    state: &State,
    instance_id: &str,
    request_key: u64,
    offset: usize,
) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    let output = lock(&live.output, "output")?;
    let Some(pending) = &output.pending else {
        return Ok(error(wire_kind::WRONG_PROCESS_KIND, "ACP pipes required"));
    };
    Ok(match pending.chunk(request_key, offset) {
        Ok(chunk) => Response::PendingRequestChunk(chunk),
        Err(failure) => error(failure.wire_kind(), "pending request could not be read"),
    })
}

fn execute_confirm_input(state: &mut State, operation_key: &str) -> Result<Response> {
    state.inputs.remove(operation_key);
    Ok(Response::Ok)
}

fn execute_input(
    state: &mut State,
    instance_id: String,
    bytes: String,
    operation_key: String,
    fingerprint: String,
) -> Result<Response> {
    if operation_key.is_empty() || operation_key.len() > 128 || fingerprint.len() > 128 {
        return Ok(error(wire_kind::INVALID_PARAMS, "invalid input identity"));
    }
    if let Some((prior_instance, prior_fingerprint, accepted)) = state.inputs.get(&operation_key) {
        return Ok(
            if prior_instance == &instance_id && prior_fingerprint == &fingerprint {
                Response::Accepted { bytes: *accepted }
            } else {
                error(wire_kind::IDEMPOTENCY_CONFLICT, "input identity reused")
            },
        );
    }
    if state.inputs.len() >= 1024 {
        return Ok(error(
            "input_pending_limit",
            "confirm previous input receipts before submitting more",
        ));
    }
    let Some(live) = state.live.get(&instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    if live.exited.load(Ordering::Acquire) {
        return Ok(error(wire_kind::PROCESS_EXITED, "process has exited"));
    }
    let bytes = STANDARD
        .decode(bytes)
        .map_err(|_| Error::InvalidBase64Input)?;
    if bytes.len() > limits::TERMINAL_PAYLOAD_BYTES {
        return Ok(error(wire_kind::INPUT_TOO_LARGE, "input exceeds 64 KiB"));
    }
    let mut output = lock(&live.output, "output")?;
    let count = match lock(&live.master, "master")?.write(&bytes) {
        Ok(count) => count,
        Err(failure) if failure.kind() == io::ErrorKind::WouldBlock => 0,
        Err(failure) => return Err(failure.into()),
    };
    if let Some(pending) = &mut output.pending {
        pending.input(&bytes[..count]);
    }
    state
        .inputs
        .insert(operation_key, (instance_id, fingerprint, count));
    Ok(Response::Accepted { bytes: count })
}

fn execute_resize(state: &State, instance_id: &str, cols: u16, rows: u16) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    let Some(resizer) = &live.resizer else {
        return Ok(error(
            wire_kind::WRONG_PROCESS_KIND,
            "terminal PTY required",
        ));
    };
    resizer.resize(cols, rows)?;
    Ok(Response::Ok)
}

fn execute_drain(state: &State, instance_id: &str, from_seq: u64) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    let output = lock(&live.output, "output")?.retention.read(
        from_seq,
        limits::TERMINAL_PAYLOAD_BYTES,
        live.exited.load(Ordering::Acquire) && live.read_done.load(Ordering::Acquire),
    );
    Ok(match output {
        Ok(output) => Response::Output(output),
        Err(failure) => error(failure.wire_kind(), "invalid output cursor"),
    })
}

fn execute_ack(state: &State, instance_id: &str, seq: u64) -> Result<Response> {
    let Some(live) = state.live.get(instance_id) else {
        return Ok(error(wire_kind::NOT_FOUND, "process not found"));
    };
    Ok(
        match lock(&live.output, "output")?.retention.acknowledge(seq) {
            Ok(()) => Response::Ok,
            Err(failure) => error(failure.wire_kind(), "invalid acknowledgement"),
        },
    )
}

fn start_readers(mut reader: Output, live: Arc<Live>, shared: Arc<Mutex<State>>, key: String) {
    let reading = Arc::clone(&live);
    thread::spawn(move || {
        let mut bytes = [0; 16384];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    let Ok(mut output) = reading.output.lock() else {
                        log::event("ptyd.output_lock_poisoned", &[]);
                        break;
                    };
                    if let Some(pending) = &mut output.pending {
                        pending.output(&bytes[..count]);
                    }
                    if output.retention.append(&bytes[..count]).is_err() {
                        log::event("ptyd.output_offset_exhausted", &[]);
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if reader.wait_readable(Duration::from_secs(1)).is_err() {
                        break;
                    }
                }
                // EIO is how a pseudoterminal ends when its child exits; any other error ends the reader too.
                Err(_) => break,
            }
        }
        reading.read_done.store(true, Ordering::Release);
    });
    thread::spawn(move || {
        loop {
            let exit = match live.child.lock() {
                Ok(mut child) => child.try_wait(),
                Err(_) => {
                    log::event("ptyd.child_lock_poisoned", &[]);
                    live.exited.store(true, Ordering::Release);
                    break;
                }
            };
            match exit {
                Ok(Some(status)) => {
                    let guarded_code = live.guard.as_ref().and_then(|control| {
                        let Ok(mut control) = control.lock() else {
                            log::event("ptyd.guard_lock_poisoned", &[]);
                            return None;
                        };
                        control.exit_code()
                    });
                    let code = guarded_code.unwrap_or_else(|| status.code().unwrap_or(-1));
                    let Ok(mut state) = shared.lock() else {
                        log::event("ptyd.state_lock_poisoned", &[]);
                        live.exited.store(true, Ordering::Release);
                        break;
                    };
                    if let Some(instance) = state.registry.instances.get_mut(&key) {
                        instance.exit_code = Some(code);
                    }
                    // Published after the status, under the lock a `List` takes, so a reader
                    // that sees the process exited also finds its exit status.
                    live.exited.store(true, Ordering::Release);
                    if state.registry.save(&state.directory).is_err() {
                        log::event("ptyd.registry_write_failed", &[]);
                    }
                    break;
                }
                Err(error) => {
                    log::failure("ptyd.child_wait_failed", &error);
                    live.exited.store(true, Ordering::Release);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_file_is_removed_when_the_supervisor_stops() {
        let path = std::env::temp_dir().join(format!("soyeht-endpoint-{}", std::process::id()));
        std::fs::write(&path, "endpoint").unwrap();
        drop(EndpointFile(path.clone()));
        assert!(!path.exists());
        // Removing a file that is already gone is not an error.
        drop(EndpointFile(path));
    }

    #[test]
    fn connection_failures_are_logged_by_variant_without_message_text() {
        let io = Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, "/private/path"));
        assert_eq!(error_label(&io), "Io::BrokenPipe");
        let remote = Error::Remote {
            kind: "kind".into(),
            message: "secret detail".into(),
        };
        assert_eq!(error_label(&remote), "Remote");
        assert_eq!(error_label(&Error::PoisonedLock("state")), "PoisonedLock");
        assert_eq!(
            error_label(&Error::ConnectionSuperseded),
            "ConnectionSuperseded"
        );
    }
}
