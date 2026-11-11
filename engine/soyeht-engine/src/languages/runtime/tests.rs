use super::*;
use crate::process_start::Started;
use crate::test_panes::{Answering, apply_until_idle, core_with_agent, quiet_read};
use soyeht_protocol::{DiagnosticSeverity, EditorLanguage, TextPosition, TextRange};
use soyeht_ptyd::wire::Response;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// A core with a running language server that holds the pending write `op` (four bytes).
fn core_with_a_pending_write() -> (Core, std::path::PathBuf, String) {
    let (mut core, directory) = core_with_agent(&[]);
    let path = directory.to_string_lossy().into_owned();
    // Started for a trusted folder, by an engine that recorded no trust (see `trust_recorded`).
    let mut server = Server::new("workspace", &path, EditorLanguage::Rust).unwrap();
    server.trusted = true;
    let key = core.languages.insert(server).unwrap();
    let spawn = language(&core.languages, &key).unwrap().spawn().unwrap();
    let instance = Instance {
        instance_id: "language-process".into(),
        supervisor_id: "answering".into(),
        spawn,
        pid: 9,
        closed: false,
        exit_code: None,
    };
    let server = language_mut(&mut core.languages, &key).unwrap();
    server.adopt(&instance, &instance.spawn).unwrap();
    server
        .pending_writes
        .insert("op".into(), STANDARD.encode("abcd"));
    (core, directory, key)
}

#[test]
fn the_failure_of_a_write_the_server_no_longer_holds_does_not_close_the_new_server() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    // The server was restarted: the write is gone with the process it was sent to.
    language_mut(&mut core.languages, &key)
        .unwrap()
        .pending_writes
        .clear();

    core.settle_language_write(&key, "op".into(), 4, Err(errors::ptyd_unavailable("lost")));
    assert_ne!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Unavailable
    );
    assert!(
        language(&core.languages, &key)
            .unwrap()
            .instance_id
            .is_some()
    );

    // The same failure of a write it does hold does leave it unavailable.
    language_mut(&mut core.languages, &key)
        .unwrap()
        .pending_writes
        .insert("op".into(), STANDARD.encode("abcd"));
    core.settle_language_write(&key, "op".into(), 4, Err(errors::ptyd_unavailable("lost")));
    assert_eq!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Unavailable
    );
}

#[test]
fn a_write_with_no_job_is_sent_again_by_the_tick_and_once() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    core.background = crate::offload::Background::default();
    let inputs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&inputs);
    core.link_supervisor(Box::new(Answering(move |request| {
        Ok(match request {
            Request::Input { .. } => {
                counted.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(300));
                Response::Accepted { bytes: 4 }
            }
            Request::Drain { from_seq, .. } => quiet_read(from_seq),
            _ => Response::Ok,
        })
    })));

    // Its first job ended without a receipt (its link was replaced): nothing is running now.
    core.advance_language_servers();
    // A tick while the job runs sends nothing more.
    core.advance_language_servers();
    apply_until_idle(&mut core);

    assert_eq!(inputs.load(Ordering::SeqCst), 1);
    assert!(
        language(&core.languages, &key)
            .unwrap()
            .pending_writes
            .is_empty()
    );
}

#[test]
fn a_server_with_nothing_to_do_waits_for_a_notice_when_the_watch_is_heard() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    let wait = |core: &Core| {
        let mut next = crate::tick_due::Next::default();
        super::super::watching::due(core, &mut next);
        next.soonest()
    };
    // Without the watch the server is read on every run, as it always was.
    assert_eq!(wait(&core), Some(std::time::Duration::ZERO));

    core.watch.pretend_active();
    // A write that no job carries is work.
    assert_eq!(wait(&core), Some(std::time::Duration::ZERO));
    language_mut(&mut core.languages, &key)
        .unwrap()
        .pending_writes
        .clear();
    assert_eq!(wait(&core), None, "nothing is announced, nothing is read");
    assert!(super::super::watching::instances(&core).contains(&"language-process".to_owned()));

    // A notice of the supervisor is work, and so is a read that ended.
    core.watch.pretend_notice("language-process", 5);
    assert_eq!(wait(&core), Some(std::time::Duration::ZERO));
}

/// What the supervisor was asked to close.
type Closed = Arc<std::sync::Mutex<Vec<String>>>;

/// A core with a running language server and a supervisor that records the closes it is asked
/// for; jobs run on the pool.
fn core_recording_closes() -> (Core, std::path::PathBuf, String, Closed) {
    let (mut core, directory, key) = core_with_a_pending_write();
    let closed = link_recording_closes(&mut core);
    (core, directory, key, closed)
}

/// Links a supervisor that records the closes it is asked for; jobs run on the pool.
fn link_recording_closes(core: &mut Core) -> Closed {
    core.background = crate::offload::Background::default();
    let closed = Closed::default();
    let record = Arc::clone(&closed);
    core.link_supervisor(Box::new(Answering(move |request| {
        if let Request::Close { instance_id } = request {
            record.lock().unwrap().push(instance_id);
        }
        Ok(Response::Ok)
    })));
    closed
}

fn language_process(core: &Core, key: &str, id: &str) -> Instance {
    Instance {
        instance_id: id.into(),
        supervisor_id: "answering".into(),
        spawn: language(&core.languages, key).unwrap().spawn().unwrap(),
        pid: 9,
        closed: false,
        exit_code: None,
    }
}

#[test]
fn a_process_the_server_already_adopted_is_not_closed_as_an_orphan() {
    let (mut core, _directory, key, closed) = core_recording_closes();

    // The start was for a server that has been restarted since, which the reconciliation of a new
    // link had already given this very process.
    let adopted = language_process(&core, &key, "language-process");
    core.finish_language_start(&key, "old-identity", Started::Process(adopted));
    apply_until_idle(&mut core);
    assert!(closed.lock().unwrap().is_empty());

    let stray = language_process(&core, &key, "stray");
    core.finish_language_start(&key, "old-identity", Started::Process(stray));
    apply_until_idle(&mut core);
    assert_eq!(*closed.lock().unwrap(), ["stray"]);
}

#[test]
fn the_failure_of_a_start_of_a_server_restarted_meanwhile_is_not_the_new_servers() {
    let (mut core, _directory, key, _closed) = core_recording_closes();
    let old_identity = language(&core.languages, &key)
        .unwrap()
        .instance_key
        .clone();
    language_mut(&mut core.languages, &key)
        .unwrap()
        .restart("restarted")
        .unwrap();
    let failure = || Started::Failed(errors::ptyd_unavailable("refused"));

    core.finish_language_start(&key, &old_identity, failure());
    assert_ne!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Unavailable
    );

    // The failure of the start the server is waiting for does stop it.
    let waiting_for = language(&core.languages, &key)
        .unwrap()
        .instance_key
        .clone();
    core.finish_language_start(&key, &waiting_for, failure());
    assert_eq!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Unavailable
    );
}

#[test]
fn the_writes_that_are_pending_go_out_again_in_the_order_they_were_made() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    core.background = crate::offload::Background::default();
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = Arc::clone(&sent);
    core.link_supervisor(Box::new(Answering(move |request| {
        Ok(match request {
            Request::Input { bytes, .. } => {
                record
                    .lock()
                    .unwrap()
                    .push(STANDARD.decode(bytes).unwrap()[0]);
                Response::Accepted { bytes: 1 }
            }
            _ => Response::Ok,
        })
    })));
    // Thirty writes made one after the other, as `initialized`, `didOpen` and `didChange` are.
    let server = language_mut(&mut core.languages, &key).unwrap();
    server.pending_writes.clear();
    for number in 0..30u8 {
        let operation = server.next_write_key();
        server
            .pending_writes
            .insert(operation, STANDARD.encode([number]));
    }

    core.resume_language_writes(&key).unwrap();
    apply_until_idle(&mut core);

    assert_eq!(*sent.lock().unwrap(), (0..30u8).collect::<Vec<_>>());
}

#[test]
fn a_write_that_is_on_its_way_is_not_decoded_again() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    core.background.busy(&format!("lsp-write:{key}:op"));

    // Not even a stored write that cannot be decoded is looked at: the tick asks for every
    // pending write, and the ones with a job cost nothing.
    core.deliver_language_write(&key, "op".into(), "not base64!".into())
        .unwrap();
}

#[test]
fn diagnostic_publication_is_bounded_after_wire_expansion() {
    let item = FileDiagnostic {
        diagnostic_id: "d".repeat(36),
        range: TextRange {
            start: TextPosition { line: 0, column: 0 },
            end: TextPosition { line: 0, column: 1 },
        },
        severity: DiagnosticSeverity::Warning,
        message: "x".repeat(soyeht_protocol::limits::DIAGNOSTIC_MESSAGE_CHARS),
        source: "server".into(),
    };
    let (items, omitted) = bounded_diagnostics("file-1", 7, vec![item; 500]).unwrap();
    let event = json!({"fileId":"file-1","revision":"7","items":items});
    assert!(
        serde_json::to_vec(&event).unwrap().len() <= soyeht_protocol::limits::JSON_BYTES - 1024
    );
    assert!(omitted > 0);
}

#[test]
fn a_server_an_earlier_engine_started_is_closed_and_started_again_once() {
    // The checkpoint of an engine that did not know project trust says nothing about it.
    let (mut core, _directory, key, closed) = core_recording_closes();
    assert!(!language(&core.languages, &key).unwrap().trust_recorded);

    assert!(core.follow_language_trust(&key, None).unwrap());
    apply_until_idle(&mut core);
    assert_eq!(*closed.lock().unwrap(), ["language-process"]);
    let server = language(&core.languages, &key).unwrap();
    assert!(server.trust_recorded);
    assert_eq!(server.generation, 2);
    assert!(server.instance_id.is_none());

    // Once: the second look finds it up to date.
    assert!(!core.follow_language_trust(&key, None).unwrap());
}

#[test]
fn revoking_trust_closes_the_process_with_the_close_that_is_retried() {
    let (mut core, _directory, key) = core_with_a_pending_write();
    core.background = crate::offload::Background::default();
    core.link_supervisor(Box::new(Answering(|request| match request {
        Request::Close { .. } => Err(remote_close_failure()),
        _ => Ok(Response::Ok),
    })));
    let server = language_mut(&mut core.languages, &key).unwrap();
    server.trusted = true;
    server.trust_recorded = true;

    // The folder has no answer in the store: the server started for a trusted folder is closed.
    assert!(core.follow_language_trust(&key, None).unwrap());
    apply_until_idle(&mut core);
    assert_eq!(
        core.supervisor_retry
            .close_retries
            .attempts("language-process"),
        1,
        "a close that failed is tried again by the tick"
    );
    assert_eq!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Restricted
    );
}

fn remote_close_failure() -> soyeht_ptyd::Error {
    soyeht_ptyd::Error::Remote {
        kind: soyeht_ptyd::wire_kind::OPERATION_FAILED.into(),
        message: "registry".into(),
    }
}

#[test]
fn a_language_server_process_no_server_owns_is_closed_when_the_supervisor_returns() {
    let (mut core, _directory, key, closed) = core_recording_closes();
    let owned = language_process(&core, &key, "owned");
    let mut stale = language_process(&core, &key, "stale");
    stale.spawn.instance_key = "replaced-by-a-restart".into();

    core.close_orphaned_agent_processes(&[owned, stale])
        .unwrap();
    apply_until_idle(&mut core);

    assert_eq!(*closed.lock().unwrap(), ["stale"]);
}

/// A search path with no language server on it: a folder of the test that holds nothing.
fn empty_search_path(directory: &std::path::Path) -> std::ffi::OsString {
    let empty = directory.join("empty-tools");
    std::fs::create_dir_all(&empty).unwrap();
    empty.into_os_string()
}

/// A process the supervisor holds for `server`, started as `argv`.
fn process_of(server: &Server, id: &str, argv: &[&str]) -> Instance {
    Instance {
        instance_id: id.into(),
        supervisor_id: "answering".into(),
        spawn: soyeht_ptyd::wire::Spawn {
            instance_key: server.instance_key.clone(),
            generation: server.generation,
            kind: soyeht_ptyd::wire::ProcessKind::PipesLsp,
            cwd: server.root.clone(),
            argv: argv.iter().map(|value| (*value).to_owned()).collect(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        },
        pid: 9,
        closed: false,
        exit_code: None,
    }
}

#[test]
fn a_server_that_cannot_start_for_its_folders_trust_is_left_unavailable_and_keeps_the_link() {
    let (mut core, directory) = core_with_agent(&[]);
    let closed = link_recording_closes(&mut core);
    // A search path with no TypeScript server on it, whatever this machine has installed.
    core.language_search_path = Some(empty_search_path(&directory));
    let path = directory.to_string_lossy().into_owned();
    let server = Server::new("workspace", &path, EditorLanguage::Typescript)
        .unwrap()
        .with_trust(false);
    let instance = process_of(
        &server,
        "typescript-process",
        &["typescript-language-server"],
    );
    let key = core.languages.insert(server).unwrap();
    language_mut(&mut core.languages, &key)
        .unwrap()
        .pending_writes
        .insert("op".into(), STANDARD.encode("abcd"));

    core.reconcile_language_servers(&[instance]).unwrap();
    apply_until_idle(&mut core);

    assert_eq!(
        language(&core.languages, &key).unwrap().state,
        soyeht_protocol::LspState::Unavailable
    );
    assert_eq!(*closed.lock().unwrap(), ["typescript-process"]);
}

#[test]
fn a_process_started_under_another_identity_is_replaced_and_keeps_the_link() {
    // A trusted server whose process was started with other arguments (another build of the
    // engine, another search path): adopting it would fail, and that must not drop the link.
    let (mut core, _directory, key, closed) = core_recording_closes();
    let stranger = {
        let server = language(&core.languages, &key).unwrap();
        process_of(server, "stranger", &["rust-analyzer", "--other"])
    };

    core.reconcile_language_servers(&[stranger]).unwrap();
    apply_until_idle(&mut core);

    // The stranger is closed, and so is the process the server had before.
    assert!(closed.lock().unwrap().contains(&"stranger".to_owned()));
    assert!(
        language(&core.languages, &key)
            .unwrap()
            .instance_id
            .is_none()
    );
    assert!(core.ptyd.is_some());
}
