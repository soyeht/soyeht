//! The avatar's pane (ADR 0166 slice 2, contract 19 section 6): starting it as the workspace's chief,
//! keeping it in step with the binding, and the binding in step with it.

use super::*;
use soyeht_store::agents;
use crate::{
    agents::{
        Changes,
        configuration::requested_option,
        state::{HarnessSwitchCheckpoint, Phase, State},
    },
    panes::Extras,
};
use soyeht_protocol::{
    PromptItem, ConfigValue, PaneCloseParams, PaneCreateParams, PaneHarnessSwitchParams, PaneMode,
    PaneRenameParams, PaneState, Role,
};

/// The avatar panes the workspace has open, alive or not.
fn open_panes<'a>(core: &'a Core, workspace_id: &'a str) -> impl Iterator<Item = &'a String> {
    core.panes
        .values()
        .filter(move |pane| !pane.stored.closed && pane.stored.pane.workspace_id == workspace_id)
        .map(|pane| &pane.stored.pane.pane_id)
        .filter(|pane_id| {
            core.agents
                .get(pane_id)
                .is_some_and(|agent| agent.state.avatar)
        })
}

/// The avatar pane the workspace has open that is alive: one that failed for good (no process and
/// no way back) is not the workspace's avatar any more, and the next assignment starts another.
pub(super) fn live(core: &Core, workspace_id: &str) -> Option<String> {
    open_panes(core, workspace_id)
        .find(|pane_id| {
            let dead = core.panes[*pane_id].stored.pane.state == PaneState::Dead;
            let faulted = core
                .agents
                .get(pane_id)
                .is_some_and(|agent| agent.state.phase == Phase::Faulted);
            !dead && !faulted
        })
        .cloned()
}

/// The `pane.create` the avatar's pane is started with: an agent of the binding's harness, model
/// and effort, named for the avatar.
fn create_request(request: &AvatarAssignParams, avatar_name: &str) -> PaneCreateParams {
    PaneCreateParams {
        workspace_id: request.workspace_id.clone(),
        mode: PaneMode::Agent,
        cwd: None,
        name: Some(avatar_name.to_owned()),
        cols: None,
        rows: None,
        idempotency_key: request.idempotency_key.clone(),
        harness: Some(request.harness.clone()),
        model: request.model.clone(),
        effort: request.effort.clone(),
        permission_mode: None,
        acknowledged_package: request.acknowledged_package.clone(),
        url: None,
        incognito: None,
        path: None,
        writing_kind: None,
        writing_section: None,
    }
}

/// What the assignment changes, in the words of the generated types.
pub(super) struct Assignment<'a> {
    pub(super) request: &'a AvatarAssignParams,
    pub(super) avatar: &'a Avatar,
    pub(super) binding: AvatarBinding,
    /// The binding the workspace had, when it had one.
    pub(super) before: Option<AvatarBinding>,
}

/// Binds the avatar and starts its pane in one change: the pane is an agent of the binding's
/// harness, named for the avatar, and the workspace's `chief` from its first session. The lead
/// prompt of `pane.role.set` is not queued: the persona says what the avatar is for. A pane of the
/// workspace that failed for good, or that cannot be switched, is closed in the same call (a
/// workspace has one avatar pane), after the checks that can refuse the new one, and the prompts it
/// had queued go to the new pane, in order; the conversation does not.
pub(super) fn start(
    core: &mut Core,
    connection: &str,
    params: &Value,
    assignment: Assignment<'_>,
) -> Result<Outcome> {
    let create = create_request(assignment.request, &assignment.avatar.name);
    core.check_agent_pane(connection, &create)?;
    let carried = queued_prompts(core, &assignment.binding.workspace_id);
    close_open(core, connection, &assignment.binding.workspace_id)?;
    let binding = assignment.binding;
    let event = binding_event(&binding)?;
    let applied = binding.clone();
    let prompts: Vec<PromptItem> = carried.iter().map(|(_, prompt)| prompt.clone()).collect();
    let extras = Extras {
        setup: Box::new(move |state: &mut State| {
            state.avatar = true;
            state.mcp_role = Role::Chief;
            state.restore_queue(prompts);
        }),
        reply: Box::new(move |pane_id| {
            let result = AvatarAssignResult {
                binding,
                pane_id: Some(pane_id.to_owned()),
            };
            Ok((model::to_value(&result)?, vec![event]))
        }),
        apply: Box::new(move |tx, pane_id| {
            library::bind_in(tx, &applied)?;
            // The queue rows belong to the pane: the closed one gives them up, and the new one has them.
            for (old_pane, prompt) in &carried {
                agents::remove_prompt(tx, old_pane, &prompt.prompt_id)?;
                agents::enqueue_prompt(tx, pane_id, prompt)?;
            }
            Ok(())
        }),
    };
    core.create_process_pane(
        connection,
        (method::AVATAR_ASSIGN, params),
        &create,
        Some(extras),
    )
}

/// The prompts the open avatar panes of the workspace have queued, with the pane each is queued in.
fn queued_prompts(core: &Core, workspace_id: &str) -> Vec<(String, PromptItem)> {
    open_panes(core, workspace_id)
        .filter_map(|pane_id| core.agents.get(pane_id).map(|agent| (pane_id, &agent.state)))
        .flat_map(|(pane_id, state)| {
            state
                .queue
                .iter()
                .map(move |prompt| (pane_id.clone(), prompt.clone()))
        })
        .collect()
}

fn binding_event(binding: &AvatarBinding) -> Result<(String, Value)> {
    model::pending(&AvatarChangedDraft {
        avatar_id: binding.avatar_id.clone(),
        avatar: None,
        workspace_id: Some(binding.workspace_id.clone()),
        binding: Some(binding.clone()),
        removed: None,
    })
}

/// Whether the pane's harness cannot be switched and will not become switchable by itself: it asks for
/// a sign-in or failed. `pane.harness.switch` refuses such a pane, so the assignment replaces it. A pane
/// that is coming back (`Recovering`, for seconds after a process was lost) is not replaced: the
/// switch answers `pane_busy` and the caller tries again.
fn refuses_switch(state: &State) -> bool {
    matches!(state.phase, Phase::NeedsLogin | Phase::Faulted)
}

/// Closes the avatar panes the workspace has open.
fn close_open(core: &mut Core, connection: &str, workspace_id: &str) -> Result<()> {
    let panes: Vec<String> = open_panes(core, workspace_id).cloned().collect();
    for pane_id in panes {
        let close = PaneCloseParams {
            pane_id,
            idempotency_key: Uuid::new_v4().to_string(),
        };
        core.pane_edit(connection, method::PANE_CLOSE, &model::to_value(&close)?)?;
    }
    Ok(())
}

/// Records the binding of a workspace that has its avatar pane open, and brings the pane to it:
/// a harness that is not the pane's goes through `pane.harness.switch` (a pane that cannot be
/// switched is replaced), and the model and effort the session announces and that differ, with a
/// persona or avatar that changed, are recorded on the pane in one change (the model and effort are
/// applied one at a time when the pane is idle, the persona is owed to its next prompt, also when a
/// switch runs and fails). The pane is changed
/// first, so a refusal leaves the binding as it was. While a switch runs the binding keeps the
/// harness, model and effort the pane has: the end of the switch records the new ones.
pub(super) fn update(
    core: &mut Core,
    connection: &str,
    params: &Value,
    pane_id: &str,
    assignment: Assignment<'_>,
) -> Result<Outcome> {
    let state = core.agent_state(pane_id)?;
    let switches = state.launch.id != assignment.binding.harness;
    if switches && refuses_switch(&state) {
        return start(core, connection, params, assignment);
    }
    let Assignment {
        request,
        avatar,
        mut binding,
        before,
    } = assignment;
    if switches {
        let switch = PaneHarnessSwitchParams {
            pane_id: pane_id.to_owned(),
            harness: binding.harness.clone(),
            model: binding.model.clone(),
            effort: binding.effort.clone(),
            acknowledged_package: request.acknowledged_package.clone(),
            idempotency_key: Uuid::new_v4().to_string(),
        };
        core.agent_harness_switch(connection, &model::to_value(&switch)?)?;
        if let Some(before) = &before {
            binding.harness.clone_from(&before.harness);
            binding.model.clone_from(&before.model);
            binding.effort.clone_from(&before.effort);
        }
    }
    // What the pane is changed by, apart from the harness: in one change. The persona is owed
    // whether or not a switch runs, which may fail and leave the pane on the persona it had.
    let persona_changed = before.as_ref().is_none_or(|before| {
        before.persona != binding.persona || before.avatar_id != binding.avatar_id
    });
    let mut state = core.agent_state(pane_id)?;
    let mut changed = !switches && request_options(&mut state, &binding);
    if persona_changed {
        crate::agents::persona::owe(&mut state);
        changed = true;
    }
    if changed {
        let saved = core.pane(pane_id)?.stored.clone();
        core.commit_agent(saved, state, Changes::default(), None, json!({}))?;
    }
    let name = avatar.name.clone();
    let event = binding_event(&binding)?;
    let result = model::to_value(&AvatarAssignResult {
        binding: binding.clone(),
        pane_id: Some(pane_id.to_owned()),
    })?;
    let reply = core
        .mutate(connection, method::AVATAR_ASSIGN, params, |tx| {
            library::bind_in(tx, &binding)?;
            Ok((result, vec![event]))
        })?
        .result;
    rename(core, connection, pane_id, &name);
    Ok(Outcome::Reply(reply))
}

/// Asks for the model and effort of the binding that the session does not have: both go to the
/// pane's requested options, which the engine applies one at a time when the pane is idle. Only a
/// selector the session announces now is asked for: the engine stops the pane's queue while it holds
/// a request for a selector the session does not offer, so a value for one that is not there (the
/// model the session has now offers no effort) stays in the binding and is not asked for.
fn request_options(state: &mut State, binding: &AvatarBinding) -> bool {
    let mut changed = false;
    for (key, wanted) in [("model", &binding.model), ("effort", &binding.effort)] {
        let Some(wanted) = wanted else { continue };
        let wanted = ConfigValue::V0(wanted.clone());
        let current = requested_option(&state.options, key).map(|option| &option.current_value);
        let differs = current.is_some_and(|current| current != &wanted);
        if differs && state.requested_options.get(key) != Some(&wanted) {
            state.requested_options.insert(key.to_owned(), wanted);
            changed = true;
        }
    }
    changed
}

/// Names the pane for the avatar. The change is made already: a pane that cannot be renamed keeps
/// its name, and the kind of the failure is logged.
fn rename(core: &mut Core, connection: &str, pane_id: &str, name: &str) {
    if core.panes[pane_id].stored.pane.name == name {
        return;
    }
    let renamed = PaneRenameParams {
        pane_id: pane_id.to_owned(),
        name: name.to_owned(),
        idempotency_key: Uuid::new_v4().to_string(),
    };
    let params = model::to_value(&renamed)
        .and_then(|params| core.pane_edit(connection, method::PANE_RENAME, &params));
    if let Err(error) = params {
        soyeht_platform::log::failure("engine.avatar_pane_rename_failed", &error);
    }
}

/// The panes that carry the avatar's name are named for it again after it was renamed.
pub(super) fn follow_rename(core: &mut Core, connection: &str, avatar: &Avatar) {
    let bindings = match library::bindings(&core.store) {
        Ok(bindings) => bindings,
        Err(error) => {
            soyeht_platform::log::failure(
                "engine.avatar_pane_rename_failed",
                &errors::storage(error),
            );
            return;
        }
    };
    for binding in bindings.iter().filter(|b| b.avatar_id == avatar.avatar_id) {
        let panes: Vec<String> = open_panes(core, &binding.workspace_id).cloned().collect();
        for pane_id in panes {
            rename(core, connection, &pane_id, &avatar.name);
        }
    }
}

/// What a switch of the avatar pane's harness chose: the harness, and the model and effort it asked
/// for (the checkpoint of the switch holds them).
pub(crate) struct Choice {
    harness: String,
    model: Option<String>,
    effort: Option<String>,
}

impl Choice {
    /// The choice of the switch `checkpoint` describes, when the pane is an avatar's.
    pub(crate) fn of(state: &State, checkpoint: &HarnessSwitchCheckpoint) -> Option<Self> {
        state.avatar.then(|| Self {
            harness: checkpoint.to_harness.clone(),
            model: checkpoint.model.clone(),
            effort: checkpoint.effort.clone(),
        })
    }
}

/// A switch of the avatar pane's harness is the workspace's avatar thinking with another harness:
/// the binding says so too, so the two do not diverge. The switch is made already, so a failure is
/// logged by its kind and the binding keeps the harness it had.
pub(crate) fn follow_switch(core: &mut Core, workspace_id: &str, choice: Choice) {
    if let Err(error) = record_switch(core, workspace_id, choice) {
        soyeht_platform::log::failure("engine.avatar_binding_follow_failed", &error);
    }
}

fn record_switch(core: &mut Core, workspace_id: &str, choice: Choice) -> Result<()> {
    let Some(mut binding) = library::binding(&core.store, workspace_id).map_err(errors::storage)?
    else {
        return Ok(());
    };
    let revision: u64 = binding
        .revision
        .parse()
        .map_err(|_| errors::internal("A stored binding has no revision"))?;
    if (&binding.harness, &binding.model, &binding.effort)
        == (&choice.harness, &choice.model, &choice.effort)
    {
        // The assignment that started the switch recorded it already.
        return Ok(());
    }
    binding.harness = choice.harness;
    binding.model = choice.model;
    binding.effort = choice.effort;
    binding.revision = (revision + 1).to_string();
    let event = binding_event(&binding)?;
    let mutation = soyeht_store::database::Store::mutate(
        &mut core.store,
        "engine",
        &Uuid::new_v4().to_string(),
        method::AVATAR_ASSIGN,
        |tx| {
            library::bind_in(tx, &binding)?;
            Ok((json!({}), vec![event]))
        },
    )
    .map_err(errors::storage)?;
    core.publish(&mutation.events)
}
