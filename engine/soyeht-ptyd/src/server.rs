use crate::{
    Error, Result, pending::Pending, process, registry::Registry, retention::Retention, wire::*,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use soyeht_protocol::{
    frame::{read_json, write_json},
    limits,
};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::{
    fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

struct Live {
    master: Mutex<File>,
    child: Mutex<Child>,
    output: Mutex<OutputState>,
    guard: Option<Mutex<UnixStream>>,
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
    active: Option<(String, UnixStream)>,
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

fn send(stream: &mut UnixStream, response: &Response) -> Result<()> {
    write_json(stream, response)?;
    Ok(())
}

pub fn run(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("ptyd.lock"))?;
    // SAFETY: lock is a live owned file, kept open for the server's lifetime.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(Error::AlreadyRunning);
    }
    let socket = directory.join("ptyd.sock");
    match fs::symlink_metadata(&socket) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(&socket)?,
        Ok(_) => return Err(Error::SocketPathNotSocket),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let registry = Registry::load(directory)?;
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let state = Arc::new(Mutex::new(State {
        directory: directory.to_owned(),
        supervisor_id: Uuid::new_v4().to_string(),
        registry,
        live: BTreeMap::new(),
        active: None,
        inputs: BTreeMap::new(),
    }));
    println!("{{\"event\":\"ptyd.ready\",\"version\":{VERSION}}}");
    for incoming in listener.incoming() {
        let stream = incoming?;
        let state = Arc::clone(&state);
        thread::spawn(move || {
            let _ = connection(stream, state);
        });
    }
    Ok(())
}

fn connection(mut stream: UnixStream, shared: Arc<Mutex<State>>) -> Result<()> {
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
        return send(&mut stream, &error("unauthorized", "hello required"));
    };
    let connection_id = Uuid::new_v4().to_string();
    {
        let mut state = lock(&shared, "state")?;
        if version != VERSION {
            return send(
                &mut stream,
                &error("version_unsupported", "incompatible supervisor protocol"),
            );
        }
        if engine_generation < state.registry.engine_generation {
            return send(
                &mut stream,
                &error("stale_engine_generation", "engine generation is stale"),
            );
        }
        if engine_generation == state.registry.engine_generation && state.active.is_some() {
            return send(
                &mut stream,
                &error("engine_connected", "an engine is already connected"),
            );
        }
        let prior_generation = state.registry.engine_generation;
        state.registry.engine_generation = engine_generation;
        if let Err(error) = state.registry.save(&state.directory) {
            state.registry.engine_generation = prior_generation;
            return Err(error);
        }
        if let Some((_, old)) = state.active.take() {
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
                .unwrap_or_else(|e| error("operation_failed", &e.to_string()));
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
        Request::Hello { .. } => Ok(error("already_authenticated", "hello was already accepted")),
        Request::List => Ok(Response::Instances(
            state.registry.instances.values().cloned().collect(),
        )),
        Request::Spawn(config) => {
            if let Some(existing) = state.registry.instances.get(&config.instance_key) {
                return Ok(if existing.spawn == config {
                    Response::Instance(existing.clone())
                } else {
                    error("idempotency_conflict", "spawn key has different parameters")
                });
            }
            let instance_id = Uuid::new_v4().to_string();
            let process::Spawned {
                child,
                input,
                output: reader,
                pid,
                guard,
            } = match process::spawn(&config, &state.directory, &instance_id) {
                Ok(spawned) => spawned,
                Err(failure) if config.kind == ProcessKind::Pty => {
                    // PTY setup either never starts a child or kills and reaps
                    // it before returning an error. No process needs adoption.
                    return Ok(error("spawn_failed", &failure.to_string()));
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
            if let Err(error) = state.registry.save(&state.directory) {
                state
                    .registry
                    .instances
                    .remove(&instance.spawn.instance_key);
                let mut child = lock(&live.child, "child")?;
                process::terminate_group(child.id());
                let _ = child.wait();
                return Err(error);
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
        Request::Kill { instance_id } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
            };
            let mut child = lock(&live.child, "child")?;
            if child.try_wait()?.is_none() {
                process::terminate_group(child.id());
                let _ = child.wait();
            }
            // Keep the live output and registry entry until Close. ACP kill
            // stops a command but leaves its output available to the caller.
            Ok(Response::Ok)
        }
        Request::Close { instance_id } => {
            if let Some(instance) = state
                .registry
                .instances
                .values_mut()
                .find(|i| i.instance_id == instance_id)
            {
                let key = instance.spawn.instance_key.clone();
                let was_closed = instance.closed;
                instance.closed = true;
                if !was_closed && let Err(error) = state.registry.save(&state.directory) {
                    let restored = state
                        .registry
                        .instances
                        .get_mut(&key)
                        .ok_or(Error::RegistryInstanceMissing)?;
                    restored.closed = false;
                    return Err(error);
                }
                if let Some(live) = state.live.remove(&instance_id) {
                    let mut child = lock(&live.child, "child")?;
                    if child.try_wait()?.is_none() {
                        process::terminate_group(child.id());
                        let _ = child.wait();
                    }
                    live.exited.store(true, Ordering::Release);
                }
            }
            Ok(Response::Ok)
        }
        Request::PendingInputs => Ok(Response::InputReceipts(
            state.inputs.keys().cloned().collect(),
        )),
        Request::PendingRequests { instance_id } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
            };
            let output = lock(&live.output, "output")?;
            let Some(pending) = &output.pending else {
                return Ok(error("wrong_process_kind", "ACP pipes required"));
            };
            let (items, recovery_gap) = pending.list();
            Ok(Response::PendingRequests {
                items,
                recovery_gap,
            })
        }
        Request::PendingRequestChunk {
            instance_id,
            request_key,
            offset,
        } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
            };
            let output = lock(&live.output, "output")?;
            let Some(pending) = &output.pending else {
                return Ok(error("wrong_process_kind", "ACP pipes required"));
            };
            Ok(match pending.chunk(request_key, offset) {
                Ok(chunk) => Response::PendingRequestChunk(chunk),
                Err(failure) => error(failure.wire_kind(), "pending request could not be read"),
            })
        }
        Request::ConfirmInput { operation_key } => {
            state.inputs.remove(&operation_key);
            Ok(Response::Ok)
        }
        Request::Input {
            instance_id,
            bytes,
            operation_key,
            fingerprint,
        } => {
            if operation_key.is_empty() || operation_key.len() > 128 || fingerprint.len() > 128 {
                return Ok(error("invalid_params", "invalid input identity"));
            }
            if let Some((prior_instance, prior_fingerprint, accepted)) =
                state.inputs.get(&operation_key)
            {
                return Ok(
                    if prior_instance == &instance_id && prior_fingerprint == &fingerprint {
                        Response::Accepted { bytes: *accepted }
                    } else {
                        error("idempotency_conflict", "input identity reused")
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
                return Ok(error("not_found", "process not found"));
            };
            if live.exited.load(Ordering::Acquire) {
                return Ok(error("process_exited", "process has exited"));
            }
            let bytes = STANDARD
                .decode(bytes)
                .map_err(|_| Error::InvalidBase64Input)?;
            if bytes.len() > limits::TERMINAL_PAYLOAD_BYTES {
                return Ok(error("input_too_large", "input exceeds 64 KiB"));
            }
            let mut output = lock(&live.output, "output")?;
            let count = match lock(&live.master, "master")?.write(&bytes) {
                Ok(count) => count,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => 0,
                Err(e) => return Err(e.into()),
            };
            if let Some(pending) = &mut output.pending {
                pending.input(&bytes[..count]);
            }
            state
                .inputs
                .insert(operation_key, (instance_id, fingerprint, count));
            Ok(Response::Accepted { bytes: count })
        }
        Request::Resize {
            instance_id,
            cols,
            rows,
        } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
            };
            if live.guard.is_some() {
                return Ok(error("wrong_process_kind", "terminal PTY required"));
            }
            let master = lock(&live.master, "master")?;
            process::resize(&master, cols, rows)?;
            Ok(Response::Ok)
        }
        Request::Drain {
            instance_id,
            from_seq,
        } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
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
        Request::Ack { instance_id, seq } => {
            let Some(live) = state.live.get(&instance_id) else {
                return Ok(error("not_found", "process not found"));
            };
            Ok(
                match lock(&live.output, "output")?.retention.acknowledge(seq) {
                    Ok(()) => Response::Ok,
                    Err(failure) => error(failure.wire_kind(), "invalid acknowledgement"),
                },
            )
        }
    }
}

fn start_readers(mut reader: File, live: Arc<Live>, shared: Arc<Mutex<State>>, key: String) {
    let reading = Arc::clone(&live);
    thread::spawn(move || {
        let mut bytes = [0; 16384];
        loop {
            match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(count) => {
                    let Ok(mut output) = reading.output.lock() else {
                        eprintln!("{{\"event\":\"ptyd.output_lock_poisoned\"}}");
                        break;
                    };
                    if let Some(pending) = &mut output.pending {
                        pending.output(&bytes[..count]);
                    }
                    if output.retention.append(&bytes[..count]).is_err() {
                        eprintln!("{{\"event\":\"ptyd.output_offset_exhausted\"}}");
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    let mut descriptor = libc::pollfd {
                        fd: reader.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: reader owns this descriptor throughout poll; the
                    // one-element array stays valid until the call returns.
                    let ready = unsafe { libc::poll(&mut descriptor, 1, 1000) };
                    if ready < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted
                    {
                        break;
                    }
                }
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
                    eprintln!("{{\"event\":\"ptyd.child_lock_poisoned\"}}");
                    live.exited.store(true, Ordering::Release);
                    break;
                }
            };
            match exit {
                Ok(Some(status)) => {
                    live.exited.store(true, Ordering::Release);
                    let guarded_code = live.guard.as_ref().and_then(|control| {
                        let Ok(mut control) = control.lock() else {
                            eprintln!("{{\"event\":\"ptyd.guard_lock_poisoned\"}}");
                            return None;
                        };
                        crate::pipe_guard::exit_code(&mut control)
                    });
                    let code = guarded_code.unwrap_or_else(|| status.code().unwrap_or(-1));
                    let Ok(mut state) = shared.lock() else {
                        eprintln!("{{\"event\":\"ptyd.state_lock_poisoned\"}}");
                        break;
                    };
                    if let Some(instance) = state.registry.instances.get_mut(&key) {
                        instance.exit_code = Some(code);
                    }
                    if state.registry.save(&state.directory).is_err() {
                        eprintln!("{{\"event\":\"ptyd.registry_write_failed\"}}");
                    }
                    break;
                }
                Err(_) => {
                    live.exited.store(true, Ordering::Release);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
            }
        }
    });
}
