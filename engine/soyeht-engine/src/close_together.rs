//! The close of many processes at once, for a workspace that closes (ADR 0171).

use crate::core::Core;
use soyeht_ptyd::wire::{MAX_CLOSE_MANY, Request};

impl Core {
    /// Closes the processes `instance_ids` of closed panes. A supervisor that closes several
    /// processes in one request (ADR 0171) gets them in one job, which writes its registry once
    /// and waits for all of them together; any other, or a batch that fails, gets one close for
    /// each, which settles and retries each by itself.
    pub(crate) fn close_processes(&mut self, instance_ids: Vec<String>) {
        let supervisor = match self.supervisor() {
            Ok(supervisor) if supervisor.closes_together() && instance_ids.len() > 1 => supervisor,
            // One at a time: each close settles itself, on a lost link too.
            _ => {
                for instance_id in &instance_ids {
                    self.close_process(instance_id);
                }
                return;
            }
        };
        for instance_ids in instance_ids.chunks(MAX_CLOSE_MANY) {
            let instance_ids = instance_ids.to_vec();
            let refs: Vec<&str> = instance_ids.iter().map(String::as_str).collect();
            // A size or an acknowledgement still queued for a process would find it gone.
            for instance_id in &refs {
                self.forget_later_for(instance_id);
            }
            let job = format!("close-many:{}", instance_ids.join(","));
            if !self.background.accepts(&job) {
                continue;
            }
            let turn = self.turn(&refs);
            let (used, sending) = (supervisor.clone(), supervisor.clone());
            let (asked, settling) = (instance_ids.clone(), instance_ids.clone());
            let started = self.spawn_background(
                job,
                move || {
                    turn.wait();
                    sending
                        .call(Request::CloseMany {
                            instance_ids: asked,
                        })
                        .map(drop)
                },
                move |core, closed| {
                    match core.settle_supervisor(&used, closed) {
                        Ok(()) => {
                            for instance_id in &settling {
                                core.settle_close(instance_id, Ok(()));
                            }
                        }
                        Err(error) => {
                            core.settle_supervisor_call("engine.pane_close_failed", Err(error));
                            for instance_id in &settling {
                                core.close_process(instance_id);
                            }
                        }
                    }
                    Ok(())
                },
            );
            if let Err(error) = started {
                // The job never started: each process gets a close of its own, which settles and
                // retries by itself.
                self.settle_supervisor_call("engine.pane_close_failed", Err(error));
                for instance_id in &instance_ids {
                    self.close_process(instance_id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::supervisor_link::tests::core_with_unstarted_pane;
    use soyeht_ptyd::{
        Error as PtydError,
        wire::{Request, Response},
        wire_kind,
    };
    use std::sync::Arc;

    fn failed_close() -> soyeht_ptyd::Result<Response> {
        Err(PtydError::Remote {
            kind: wire_kind::OPERATION_FAILED.into(),
            message: "registry".into(),
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
            failed_close,
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
}
