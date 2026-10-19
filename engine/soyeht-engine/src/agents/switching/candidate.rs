//! The candidate process of a harness switch: spawning it, opening its session and
//! applying the requested configuration.

use super::*;

impl Core {
    pub(super) fn advance_candidate_response(
        &mut self,
        pane: &str,
        next_name: &str,
        params: &PaneHarnessSwitchParams,
        candidate: &mut Candidate,
        call: soyeht_acp::client::Call,
        response: std::result::Result<Value, soyeht_acp::RpcError>,
    ) -> Result<bool> {
        match &candidate.phase {
            CandidatePhase::Initializing if call.context == "switch:initialize" => {
                let value = response.map_err(candidate_rpc_error)?;
                let initialized = session::Initialization::parse(&value).map_err(native_error)?;
                candidate.state.fingerprint = session::fingerprint(
                    &candidate.state.binary_digest,
                    &initialized.version,
                    &json!(candidate.state.launch),
                )
                .map_err(native_error)?;
                candidate.state.initialized = Some(initialized);
                let servers = if candidate.state.mcp_enabled {
                    vec![crate::agents::mcp::declaration(
                        &self.store,
                        &self.directory,
                        &candidate.state,
                        &candidate.saved,
                    )?]
                } else {
                    Vec::new()
                };
                let request = candidate
                    .state
                    .client
                    .request(
                        "session/new",
                        crate::agents::opening::parameters(
                            &self.directory,
                            &candidate.state,
                            soyeht_acp::client::new_session_with_mcp(
                                &candidate.saved.pane.cwd,
                                &servers,
                            ),
                        )?,
                        "switch:open",
                        self.now_ms() as u64,
                    )
                    .map_err(native_error)?;
                self.write_candidate(&candidate.instance.instance_id, request)?;
                candidate.phase = CandidatePhase::Opening;
                Ok(false)
            }
            CandidatePhase::Opening if call.context == "switch:open" => {
                let value = response.map_err(candidate_rpc_error)?;
                candidate.state.session_id =
                    Some(session::session_id(&value).map_err(native_error)?);
                let options = Options::from_response(&value).map_err(native_error)?;
                if let Some(mode) = candidate.default_mode.take().filter(|mode| {
                    options.offers_mode(mode)
                        && !candidate
                            .state
                            .mode
                            .as_deref()
                            .is_some_and(|kept| options.offers_mode(kept))
                }) {
                    candidate.state.mode = Some(mode);
                }
                crate::agents::configuration::open_session(&mut candidate.state, options)?;
                let requested = requested_options(params, &candidate.state);
                if requested.is_empty() {
                    return self.begin_continuation_or_commit(pane, next_name, candidate);
                }
                self.send_next_candidate_option(candidate, requested)?;
                Ok(false)
            }
            CandidatePhase::Configuring {
                option_id,
                desired,
                remaining,
            } if call.context == "switch:config" => {
                let value = response.map_err(candidate_rpc_error)?;
                let options = candidate
                    .state
                    .options
                    .confirmed(option_id, desired, &value)
                    .map_err(native_error)?;
                crate::agents::configuration::confirmed(&mut candidate.state, options, option_id);
                if remaining.is_empty() {
                    return self.begin_continuation_or_commit(pane, next_name, candidate);
                }
                self.send_next_candidate_option(candidate, remaining.clone())?;
                Ok(false)
            }
            CandidatePhase::Continuing {
                incorporated_pending,
            } if call.context == "switch:continuation" => {
                response.map_err(candidate_rpc_error)?;
                let incorporated_pending = incorporated_pending.clone();
                self.commit_switch_candidate(pane, next_name, candidate, &incorporated_pending)?;
                Ok(true)
            }
            _ => Err(errors::harness_unavailable(
                "The selected harness answered out of order",
            )),
        }
    }

    pub(super) fn spawn_switch_candidate(
        &mut self,
        pane: &str,
        runtime: &HarnessSwitchRuntime,
    ) -> Result<Candidate> {
        let original = self.agent_state(pane)?;
        let checkpoint = errors::invariant(
            original.harness_switch.clone(),
            "Harness switch checkpoint disappeared",
        )?;
        let old_saved = self.panes[pane].stored.clone();
        let new_generation = old_saved
            .pane
            .generation
            .checked_add(1)
            .ok_or_else(|| errors::internal("Pane generation exhausted"))?;
        let mut saved = old_saved.clone();
        saved.pane.generation = new_generation;
        saved.instance_key = checkpoint.candidate_instance_key.clone();
        saved.instance_id = None;
        saved.supervisor_id = None;
        saved.pane.pid = None;
        saved.sequence_base = self.panes[pane].screen.next_offset();
        let mut state = State::new(runtime.launch.clone(), runtime.binary_digest.clone(), None)?;
        state.mode = original.mode.clone();
        state.internal_continuation = Some(InternalContinuation::default());
        state.binding = original.binding.clone();
        state.mcp_role = original.mcp_role.clone();
        state.mcp_enabled = original.mcp_enabled;
        state.permission_rules = original.permission_rules.clone();
        state.adopt_queue(&original);
        state.queue_paused = checkpoint.previous_queue_paused;
        state.next_message_seq = original.next_message_seq;
        state.has_finished_turn = original.has_finished_turn;
        let spawn = Spawn {
            instance_key: checkpoint.candidate_instance_key,
            generation: new_generation,
            kind: ProcessKind::PipesAcp,
            cwd: saved.pane.cwd.clone(),
            argv: std::iter::once(runtime.launch.command.clone())
                .chain(runtime.launch.args.clone())
                .collect(),
            env: crate::agents::environment::launch_environment_with(
                runtime.launch.environment.as_deref(),
            ),
            cols: saved.pane.cols as u16,
            rows: saved.pane.rows as u16,
        };
        let Response::Instance(instance) = self.pty_request(Request::Spawn(spawn))? else {
            return Err(errors::ptyd_unavailable("Invalid candidate spawn response"));
        };
        Ok(Candidate {
            instance,
            saved,
            state,
            decoder: Decoder::new(0),
            raw_output: Vec::new(),
            phase: CandidatePhase::Initializing,
            default_mode: crate::settings::default_agent_mode(&self.store, &runtime.launch.id)?,
        })
    }

    pub(super) fn initialize_switch_candidate(&mut self, candidate: &mut Candidate) -> Result<()> {
        let initialize = candidate
            .state
            .client
            .request(
                "initialize",
                soyeht_acp::client::initialize_params(),
                "switch:initialize",
                self.now_ms() as u64,
            )
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, initialize)
    }

    fn send_next_candidate_option(
        &mut self,
        candidate: &mut Candidate,
        mut requested: Vec<(String, String)>,
    ) -> Result<()> {
        let (public_name, requested_value) = requested.remove(0);
        let option =
            crate::agents::configuration::requested_option(&candidate.state.options, &public_name)
                .cloned()
                .ok_or_else(|| {
                    errors::harness_unavailable(format!(
                        "The selected harness does not offer {public_name}"
                    ))
                })?;
        let desired = ConfigValue::V0(requested_value);
        let session_id = errors::invariant(
            candidate.state.session_id.as_deref(),
            "Candidate configuration has no session",
        )?;
        let (method, params) = candidate
            .state
            .options
            .request(session_id, &option.id, &desired)
            .map_err(native_error)?
            .ok_or_else(|| errors::internal("A requested switch option is not native"))?;
        let request = candidate
            .state
            .client
            .request(method, params, "switch:config", self.now_ms() as u64)
            .map_err(native_error)?;
        self.write_candidate(&candidate.instance.instance_id, request)?;
        candidate.phase = CandidatePhase::Configuring {
            option_id: option.id,
            desired,
            remaining: requested,
        };
        Ok(())
    }

    pub(super) fn drain_candidate(
        &mut self,
        candidate: &mut Candidate,
    ) -> Result<
        Option<(
            soyeht_acp::client::Call,
            std::result::Result<Value, soyeht_acp::RpcError>,
        )>,
    > {
        let from = candidate.decoder.next_offset();
        let Response::Output(output) = self.pty_request(Request::Drain {
            instance_id: candidate.instance.instance_id.clone(),
            from_seq: from,
        })?
        else {
            return Err(errors::ptyd_unavailable(
                "Invalid candidate output response",
            ));
        };
        if output.from_seq != from || output.gap.is_some() {
            return Err(errors::ptyd_unavailable(
                "Candidate output is discontinuous",
            ));
        }
        let bytes = STANDARD
            .decode(output.bytes)
            .map_err(|_| errors::ptyd_unavailable("Invalid candidate output encoding"))?;
        if output.next_seq != output.from_seq + bytes.len() as u64 {
            return Err(errors::ptyd_unavailable(
                "Candidate output offsets disagree",
            ));
        }
        let mut response = None;
        if !bytes.is_empty() {
            candidate.raw_output.extend_from_slice(&bytes);
            for record in
                decode(&mut candidate.decoder, output.from_seq, &bytes).map_err(native_error)?
            {
                match record.message {
                    message @ Message::Response { .. } if response.is_none() => {
                        response = Some(
                            candidate
                                .state
                                .client
                                .response(message)
                                .map_err(native_error)?,
                        );
                    }
                    Message::Notification { .. } => {}
                    Message::Request { .. } | Message::Response { .. } => {
                        return Err(errors::harness_unavailable(
                            "The selected harness sent unexpected startup traffic",
                        ));
                    }
                }
            }
        }
        if output.exited {
            return Err(errors::harness_unavailable(
                "The selected harness exited during startup",
            ));
        }
        Ok(response)
    }

    pub(super) fn write_candidate(&mut self, instance_id: &str, message: Message) -> Result<()> {
        let encoded = message.encode().map_err(native_error)?;
        let mut offset = 0;
        while offset < encoded.len() {
            let end = (offset + limits::TERMINAL_PAYLOAD_BYTES).min(encoded.len());
            let bytes = &encoded[offset..end];
            let operation_key = format!("switch-input:{}", Uuid::new_v4());
            let Response::Accepted { bytes: accepted } = self.pty_request(Request::Input {
                instance_id: instance_id.into(),
                bytes: STANDARD.encode(bytes),
                operation_key: operation_key.clone(),
                fingerprint: format!("{:x}", Sha256::digest(bytes)),
            })?
            else {
                return Err(errors::ptyd_unavailable("Invalid candidate input receipt"));
            };
            self.pty_request(Request::ConfirmInput { operation_key })?;
            if accepted == 0 || accepted > bytes.len() {
                return Err(errors::ptyd_unavailable(
                    "Candidate did not accept its complete input",
                ));
            }
            offset += accepted;
        }
        Ok(())
    }
}
