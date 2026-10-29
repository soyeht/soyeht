//! Ending a harness switch: the continuation package, the commit, and the failure paths.

use super::*;

impl Core {
    pub(super) fn begin_continuation_or_commit(
        &mut self,
        pane: &str,
        next_name: &str,
        candidate: &mut Candidate,
    ) -> Result<bool> {
        let original = self.agent_state(pane)?;
        let Some((package, included_messages)) =
            self.continuation_package(pane, &original, next_name)?
        else {
            self.commit_switch_candidate(pane, next_name, candidate, &[])?;
            return Ok(true);
        };
        if let Some(continuation) = candidate.state.internal_continuation.as_mut() {
            continuation.record_prompt(&package);
        }
        let session_id = errors::invariant(
            candidate.state.session_id.as_deref(),
            "Candidate continuation has no session",
        )?;
        let request = candidate
            .state
            .client
            .request(
                "session/prompt",
                soyeht_acp::client::prompt(session_id, &package),
                "switch:continuation",
                self.now_ms() as u64,
            )
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, request)?;
        let incorporated_pending = original
            .queue
            .iter()
            .rev()
            .take(included_messages.min(original.queue.len()))
            .map(|prompt| prompt.prompt_id.clone())
            .collect();
        candidate.phase = CandidatePhase::Continuing {
            incorporated_pending,
        };
        Ok(false)
    }

    pub(super) fn commit_switch_candidate(
        &mut self,
        pane: &str,
        next_name: &str,
        candidate: &mut Candidate,
        incorporated_pending: &[String],
    ) -> Result<()> {
        let original = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            original.harness_switch.clone(),
            "Harness switch checkpoint disappeared before commit",
        )?;
        let old_saved = self.panes[pane].stored.clone();
        let session_id = errors::invariant(
            candidate.state.session_id.clone(),
            "Candidate session did not open",
        )?;
        candidate.state.adopt_queue(&original);
        for prompt_id in incorporated_pending {
            candidate.state.remove(prompt_id)?;
        }
        candidate.state.harness_switch = None;
        candidate.state.requested_options.clear();
        candidate.state.acknowledged_offset = candidate.decoder.next_offset();
        candidate.state.phase = Phase::Ready;
        candidate.saved.instance_id = Some(candidate.instance.instance_id.clone());
        candidate.saved.supervisor_id = Some(candidate.instance.supervisor_id.clone());
        candidate.saved.pane.pid = Some(candidate.instance.pid);
        candidate.saved.pane.source = StateSource::Protocol;
        candidate.saved.pane.confidence = 1.0;
        candidate.saved.pane.state = PaneState::Idle;
        candidate.saved.pane.phrase = format!("Restarted with {next_name}");
        // Keep a name the user chose, including one set while the switch ran;
        // a default name follows the harness.
        candidate.saved.pane.name =
            if naming::is_default_name(&old_saved.pane.name, &original.launch) {
                self.default_agent_name(
                    &old_saved.pane.workspace_id,
                    naming::harness_label(&candidate.state.launch),
                    Some(pane),
                )
            } else {
                old_saved.pane.name.clone()
            };
        if let Some(retired) = old_saved
            .instance_id
            .clone()
            .zip(old_saved.supervisor_id.clone())
            .map(|(instance_id, supervisor_id)| RetiredAgentProcess {
                instance_key: old_saved.instance_key.clone(),
                instance_id,
                supervisor_id,
            })
        {
            candidate.state.retired_processes.push(retired);
        }
        let marker_message =
            transcript::handover(&mut candidate.state, candidate.saved.pane.generation)?;
        let marker_seq = marker_message.seq.clone();
        let result = json!({
            "paneId":pane,"fromHarness":checkpoint.from_harness,
            "toHarness":checkpoint.to_harness,"newSessionId":session_id,"markerSeq":marker_seq,
        });
        let mut changes = Changes {
            completed_user: Some(Completion {
                user: checkpoint.user,
                result: Ok(result.clone()),
            }),
            retire_instance: old_saved
                .instance_id
                .clone()
                .zip(old_saved.supervisor_id.clone()),
            ..Changes::default()
        };
        // The generic pane.update event cannot carry TranscriptKind::Handover.
        // The client reloads the typed transcript on pane.harness_switch_done.
        changes.messages.push(marker_message);
        changes.events.push((
            event::PANE_HARNESS_SWITCH_DONE.into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness,"newSessionId":session_id,"markerSeq":marker_seq}),
        ));
        self.hooks.before_switch_commit(
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Candidate pane disappeared before commit"))?,
        )?;
        // The original process can emit output while its replacement starts.
        // Anchor the candidate generation only after that output is ingested;
        // the base captured at spawn is necessarily stale in that case.
        candidate.saved.sequence_base = self.panes[pane].screen.next_offset();
        let candidate_base = candidate.saved.sequence_base;
        self.commit_agent(
            candidate.saved.clone(),
            candidate.state.clone(),
            changes,
            None,
            json!({}),
        )?;
        self.agents
            .get_mut(pane)
            .ok_or_else(|| errors::internal("Candidate runtime disappeared after commit"))?
            .decoder = std::mem::replace(&mut candidate.decoder, Decoder::new(0));
        if !candidate.raw_output.is_empty() {
            self.panes
                .get_mut(pane)
                .ok_or_else(|| errors::internal("Candidate pane disappeared after commit"))?
                .screen
                .append_data(candidate_base, std::mem::take(&mut candidate.raw_output))?;
        }
        // Flow control only: a missed ack keeps output retained until the next one.
        self.send_later(Request::Ack {
            instance_id: candidate.instance.instance_id.clone(),
            seq: self.agents[pane].state.acknowledged_offset,
        });
        Ok(())
    }

    fn continuation_package(
        &self,
        pane: &str,
        state: &State,
        next_name: &str,
    ) -> Result<Option<(String, usize)>> {
        let page = self
            .store
            .agent_transcript(pane, None, limits::TRANSCRIPT_PAGE_MESSAGES)
            .map_err(errors::storage)?;
        let mut owned: Vec<(ContinuationRole, continuation::State, String)> = page
            .items
            .into_iter()
            .filter(|message| message.complete && message.kind == TranscriptKind::Message)
            .filter_map(|message| match message.role {
                TranscriptRole::User => Some((
                    ContinuationRole::User,
                    continuation::State::Delivered,
                    message.text,
                )),
                TranscriptRole::Assistant => Some((
                    ContinuationRole::Assistant,
                    continuation::State::Delivered,
                    message.text,
                )),
                TranscriptRole::System => None,
            })
            .collect();
        owned.extend(state.queue.iter().map(|prompt| {
            (
                ContinuationRole::User,
                continuation::State::Pending,
                prompt.text.clone(),
            )
        }));
        if owned.is_empty() {
            return Ok(None);
        }
        let messages = owned
            .iter()
            .map(|(role, status, text)| ContinuationMessage {
                role: *role,
                state: *status,
                text,
            })
            .collect::<Vec<_>>();
        let task = crate::tasks::ports::TaskStorePort::active_task_for_pane(&self.store, pane)
            .map_err(errors::storage)?;
        continuation::build_with_receipt(
            CONTINUATION_TEMPLATE,
            Fields {
                previous_harness: &state.launch.name,
                new_harness: next_name,
                task_title: task.as_ref().map(|task| task.title.as_str()),
                worktree_path: Some(&self.panes[pane].stored.pane.cwd),
                branch: None,
                transcript_path: None,
                messages: &messages,
            },
        )
        .map(|built| Some((built.text, built.included_messages)))
        .map_err(native_error)
    }

    pub(super) fn restore_failed_switch(
        &mut self,
        pane: &str,
        reason: &str,
        sign_in: Option<SignInChange>,
    ) -> Result<()> {
        let mut state = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            state.harness_switch.take(),
            "Failed harness switch has no checkpoint",
        )?;
        state.queue_paused = checkpoint.previous_queue_paused;
        let mut saved = self.panes[pane].stored.clone();
        saved.pane.state = if state.active.is_some() {
            PaneState::Working
        } else {
            PaneState::Idle
        };
        saved.pane.phrase = "Original agent session restored".into();
        let target = if checkpoint.to_name.is_empty() {
            &checkpoint.to_harness
        } else {
            &checkpoint.to_name
        };
        let marker_message = transcript::new_message(
            &mut state,
            saved.pane.generation,
            TranscriptRole::System,
            TranscriptKind::System,
            format!("The switch to {target} failed. The original agent session is still running."),
            true,
        )?;
        let mut changes = Changes {
            completed_user: Some(Completion {
                user: checkpoint.user,
                result: Err(errors::wire(errors::harness_switch_failed(reason))),
            }),
            sign_in,
            ..Changes::default()
        };
        transcript::put(&mut changes, pane, marker_message, "agent_message_chunk")?;
        changes.events.push((
            event::PANE_HARNESS_SWITCH_FAILED.into(),
            json!({"paneId":pane,"fromHarness":checkpoint.from_harness,
                "toHarness":checkpoint.to_harness,"reason":reason,"restored":true}),
        ));
        self.commit_agent(saved, state, changes, None, json!({}))?;
        Ok(())
    }

    pub(super) fn retire_failed_candidate(
        &mut self,
        pane: &str,
        instance: &Instance,
    ) -> Result<()> {
        // The ledger holds the process until the tick has closed it.
        let mut state = self.agent_state(pane)?;
        state.retired_processes.push(RetiredAgentProcess {
            instance_key: instance.spawn.instance_key.clone(),
            instance_id: instance.instance_id.clone(),
            supervisor_id: instance.supervisor_id.clone(),
        });
        self.commit_agent(
            self.panes[pane].stored.clone(),
            state,
            Changes::default(),
            None,
            json!({}),
        )?;
        Ok(())
    }
}
