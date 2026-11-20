//! The close of a workspace's processes together (ADR 0171).

use crate::supervisor_link::tests::core_with_unstarted_pane;
use soyeht_ptyd::{
    Error as PtydError,
    wire::{Request, Response},
    wire_kind,
};
use std::sync::Arc;

/// A supervisor that refuses the batch.
fn refused_batch() -> soyeht_ptyd::Result<Response> {
    Err(PtydError::Remote {
        kind: wire_kind::INVALID_PARAMS.into(),
        message: "batch".into(),
    })
}

/// What a supervisor that lists `features` is sent while `close_processes` closes `ids`; the
/// close of the batch is `batch_answer`, and every other request is answered `Ok`.
fn requests_to_close(
    features: &[&str],
    batch_answer: fn() -> soyeht_ptyd::Result<Response>,
    ids: &[&str],
) -> Vec<String> {
    let (mut core, _directory, _id, _calls) = core_with_unstarted_pane(Vec::new());
    core.background = crate::offload::Background::default();
    let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = Arc::clone(&sent);
    core.link_supervisor(Box::new(crate::test_panes::Featured(
        Box::new(crate::test_panes::Answering(move |request| {
            let many = matches!(request, Request::CloseMany { .. });
            record.lock().unwrap().push(format!("{request:?}"));
            if many {
                batch_answer()
            } else {
                Ok(Response::Ok)
            }
        })),
        features.iter().map(|feature| (*feature).into()).collect(),
    )));
    core.close_processes(ids.iter().map(|id| (*id).to_owned()).collect());
    crate::test_panes::apply_until_idle(&mut core);
    sent.lock().unwrap().clone()
}

#[test]
fn the_processes_of_a_closed_workspace_are_closed_in_one_request_when_the_supervisor_can() {
    let sent = requests_to_close(
        &[soyeht_ptyd::wire::FEATURE_CLOSE_MANY],
        || Ok(Response::Ok),
        &["a", "b", "c"],
    );
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("CloseMany"), "{sent:?}");
}

#[test]
fn a_supervisor_that_cannot_close_together_is_asked_for_each_process() {
    let sent = requests_to_close(&[], || Ok(Response::Ok), &["a", "b", "c"]);
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(sent.iter().all(|request| request.starts_with("Close {")));
}

#[test]
fn a_batch_the_supervisor_refuses_is_closed_one_process_at_a_time() {
    let sent = requests_to_close(
        &[soyeht_ptyd::wire::FEATURE_CLOSE_MANY],
        refused_batch,
        &["a", "b", "c"],
    );
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert!(sent[0].starts_with("CloseMany"));
    assert!(
        sent[1..]
            .iter()
            .all(|request| request.starts_with("Close {"))
    );
}
