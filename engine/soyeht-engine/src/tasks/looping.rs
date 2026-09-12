//! Durable per-pane task loop.

use super::{api::parameters, new_loop_config};
use crate::{
    agents::{Changes, state::Phase},
    core::Core,
    errors::{self, Result},
    ports::StorePort,
};
use serde_json::{Value, json};
use soyeht_protocol::{LoopConfig, LoopSetParams, LoopStopWhen, PaneState, PromptOrigin, Role};
use soyeht_store::database::Change;
use uuid::Uuid;

const LOOP_PROMPT_PREFIX: &str = "loop:";

#[derive(Clone)]
pub(crate) struct TaskLoopChange {
    pane_id: String,
    config: LoopConfig,
    next_tick_ms: u64,
}

impl TaskLoopChange {
    pub(crate) fn persist(&self, change: &Change<'_>) -> soyeht_store::Result<()> {
        change.save_loop_config(&self.pane_id, &self.config, self.next_tick_ms)
    }
}

#[derive(Clone)]
pub(crate) struct TaskNextLoop {
    pane_id: String,
    config: LoopConfig,
    next_tick_ms: u64,
}

pub(crate) fn record_task_next(
    change: &Change<'_>,
    context: Option<&TaskNextLoop>,
    empty: bool,
) -> soyeht_store::Result<Vec<(String, Value)>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };
    let mut config = context.config.clone();
    config.empty_results = Some(if empty {
        config.empty_results.unwrap_or(0).saturating_add(1)
    } else {
        0
    });
    let stopped = empty
        && config.stop_when == LoopStopWhen::ThreeEmpty
        && config.empty_results.unwrap_or(0) >= 3;
    if stopped {
        config.enabled = false;
    }
    change.save_loop_config(&context.pane_id, &config, context.next_tick_ms)?;
    Ok(stopped
        .then(|| {
            (
                "loop.stopped".into(),
                json!({"paneId":context.pane_id,"reason":"three_empty"}),
            )
        })
        .into_iter()
        .collect())
}

impl Core {
    pub(crate) fn loop_set(&mut self, connection: &str, raw: &Value) -> Result<Value> {
        let params: LoopSetParams = parameters(raw)?;
        let actor = self.task_actor(connection)?;
        if matches!(actor.role, Role::Agent | Role::Chief)
            && actor.pane_id.as_deref() != Some(params.pane_id.as_str())
        {
            return Err(errors::forbidden_role(
                "An agent may configure only its own loop",
            ));
        }
        self.agent_state(&params.pane_id)?;
        let config = new_loop_config(&params)?;
        let next_tick_ms = next_tick(self.now_ms(), config.interval)?;
        let pane_id = params.pane_id;
        let stopped = !config.enabled;
        let mutation = self.mutate(connection, "loop.set", raw, move |change| {
            change.save_loop_config(&pane_id, &config, next_tick_ms)?;
            let events = if stopped {
                vec![(
                    "loop.stopped".into(),
                    json!({"paneId":pane_id,"reason":"explicit_off"}),
                )]
            } else {
                Vec::new()
            };
            Ok((json!({"config":config}), events))
        })?;
        Ok(mutation.result)
    }

    pub(crate) fn advance_task_loops(&mut self) -> Result<()> {
        let now = self.now_ms().max(0) as u64;
        let due = self.store.due_loop_configs(now).map_err(errors::storage)?;
        for (pane, config, _) in due {
            if self.loop_can_dispatch(&pane, now)? {
                self.dispatch_loop_prompt(&pane, config, now)?;
            }
        }
        Ok(())
    }

    pub(crate) fn task_next_loop(&self, pane: &str) -> Result<Option<TaskNextLoop>> {
        let state = self.agent_state(pane)?;
        if !state
            .active
            .as_ref()
            .is_some_and(|prompt| prompt.prompt_id.starts_with(LOOP_PROMPT_PREFIX))
        {
            return Ok(None);
        }
        self.store
            .loop_config(pane)
            .map_err(errors::storage)
            .map(|value| {
                value.and_then(|(config, next_tick_ms)| {
                    config.enabled.then_some(TaskNextLoop {
                        pane_id: pane.into(),
                        config,
                        next_tick_ms,
                    })
                })
            })
    }

    fn loop_can_dispatch(&self, pane: &str, now: u64) -> Result<bool> {
        let Some(saved) = self.panes.get(pane).filter(|pane| !pane.stored.closed) else {
            return Ok(false);
        };
        let state = self.agent_state(pane)?;
        let no_task = self
            .store
            .active_task_for_pane(pane)
            .map_err(errors::storage)?
            .is_none();
        let no_question = self
            .store
            .agent_open_requests(pane)
            .map_err(errors::storage)?
            .is_empty();
        Ok(saved.stored.pane.state == PaneState::Idle
            && state.phase == Phase::Ready
            && state.active.is_none()
            && state.queue.is_empty()
            && !state.queue_paused
            && state.draft_until_ms <= now as i64
            && no_task
            && no_question)
    }

    fn dispatch_loop_prompt(&mut self, pane: &str, mut config: LoopConfig, now: u64) -> Result<()> {
        let round = config.rounds.unwrap_or(0).saturating_add(1);
        config.rounds = Some(round);
        let next_tick_ms = next_tick(now as i64, config.interval)?;
        let round_limit = config.max_rounds > 0 && round >= config.max_rounds;
        if round_limit {
            config.enabled = false;
        }
        let mut state = self.agent_state(pane)?;
        let prompt = state.enqueue_identified(
            format!("{LOOP_PROMPT_PREFIX}{}", Uuid::new_v4()),
            config.prompt.clone(),
            PromptOrigin::Automatic,
            self.timestamp(),
        )?;
        let mut events = vec![
            (
                "pane.queued".into(),
                json!({"paneId":pane,"n":state.queue.len(),"promptIds":[prompt.prompt_id],"draft":false}),
            ),
            (
                "loop.tick".into(),
                json!({"paneId":pane,"round":round,"of":config.max_rounds}),
            ),
        ];
        if round_limit {
            events.push((
                "loop.stopped".into(),
                json!({"paneId":pane,"reason":"round_limit"}),
            ));
        }
        let saved = self.panes[pane].stored.clone();
        self.commit_agent(
            saved,
            state,
            Changes {
                events,
                task_loop: Some(TaskLoopChange {
                    pane_id: pane.into(),
                    config,
                    next_tick_ms,
                }),
                ..Changes::default()
            },
            None,
            json!({}),
        )?;
        Ok(())
    }
}

fn next_tick(now_ms: i64, interval_seconds: u32) -> Result<u64> {
    let delay = u64::from(interval_seconds)
        .checked_mul(1_000)
        .ok_or_else(|| errors::invalid("Loop interval exceeds its range"))?;
    (now_ms.max(0) as u64)
        .checked_add(delay)
        .ok_or_else(|| errors::invalid("Loop deadline exceeds its range"))
}
