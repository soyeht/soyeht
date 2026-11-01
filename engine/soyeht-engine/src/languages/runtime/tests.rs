use super::*;
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
    let server = Server::new("workspace", &path, EditorLanguage::Rust).unwrap();
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
    server.adopt(&instance).unwrap();
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
fn diagnostic_publication_is_bounded_after_wire_expansion() {
    let item = FileDiagnostic {
        diagnostic_id: "d".repeat(36),
        range: TextRange {
            start: TextPosition { line: 0, column: 0 },
            end: TextPosition { line: 0, column: 1 },
        },
        severity: DiagnosticSeverity::Warning,
        message: "x".repeat(4000),
        source: "server".into(),
    };
    let (items, omitted) = bounded_diagnostics("file-1", 7, vec![item; 500]).unwrap();
    let event = json!({"fileId":"file-1","revision":"7","items":items});
    assert!(
        serde_json::to_vec(&event).unwrap().len() <= soyeht_protocol::limits::JSON_BYTES - 1024
    );
    assert!(omitted > 0);
}
