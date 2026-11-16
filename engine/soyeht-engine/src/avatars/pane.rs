//! The avatar's pane (ADR 0166 slice 2, contract 19 section 6): starting it as the workspace's chief,
//! keeping it in step with the binding, and the binding in step with it.

use super::*;
use crate::{
    agents::{Changes, configuration::requested_option, state::Phase, state::State},
    panes::Extras,
};
use soyeht_protocol::{
    ConfigValue, PaneConfigSetParams, PaneCreateParams, PaneHarnessSwitchParams, PaneMode,
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

fn named_pane(mut result: Value, pane_id: &str) -> Value {
    result["paneId"] = Value::String(pane_id.to_owned());
    result
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
/// prompt of `pane.role.set` is not queued: the persona says what the avatar is for.
pub(super) fn start(
    core: &mut Core,
    connection: &str,
    params: &Value,
    assignment: Assignment<'_>,
) -> Result<Outcome> {
    let create = create_request(assignment.request, &assignment.avatar.name);
    let binding = assignment.binding;
    let event = binding_event(&binding)?;
    let applied = binding.clone();
    let extras = Extras {
        setup: Box::new(|state: &mut State| {
            state.avatar = true;
            state.mcp_role = Role::Chief;
        }),
        reply: Box::new(move |pane_id| {
            let result = AvatarAssignResult {
                binding,
                pane_id: Some(pane_id.to_owned()),
            };
            Ok((model::to_value(&result)?, vec![event]))
        }),
        apply: Box::new(move |tx| library::bind_in(tx, &applied)),
    };
    core.create_process_pane(
        connection,
        (method::AVATAR_ASSIGN, params),
        &create,
        Some(extras),
    )
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

/// Records the binding of a workspace that has its avatar pane open, and brings the pane to it:
/// a harness that is not the pane's goes through `pane.harness.switch`, a model or effort the
/// session announces through `pane.config.set`, and a persona or avatar that changed is owed to
/// the next prompt of a harness with no system prompt (a harness with one reads it when its
/// session next opens). The pane is changed first, so a refusal of the switch or of the option
/// leaves the binding as it was.
pub(super) fn update(
    core: &mut Core,
    connection: &str,
    params: &Value,
    pane_id: &str,
    assignment: Assignment<'_>,
) -> Result<Outcome> {
    let Assignment {
        request,
        avatar,
        binding,
        before,
    } = assignment;
    let state = core.agent_state(pane_id)?;
    if state.launch.id != binding.harness {
        let switch = PaneHarnessSwitchParams {
            pane_id: pane_id.to_owned(),
            harness: binding.harness.clone(),
            model: binding.model.clone(),
            effort: binding.effort.clone(),
            acknowledged_package: request.acknowledged_package.clone(),
            idempotency_key: Uuid::new_v4().to_string(),
        };
        core.agent_harness_switch(connection, &model::to_value(&switch)?)?;
    } else {
        for (key, wanted) in [("model", &binding.model), ("effort", &binding.effort)] {
            set_option(core, connection, pane_id, key, wanted.as_deref())?;
        }
        let changed = before.as_ref().is_none_or(|before| {
            before.persona != binding.persona || before.avatar_id != binding.avatar_id
        });
        if changed {
            owe_persona(core, pane_id)?;
        }
    }
    let name = avatar.name.clone();
    let event = binding_event(&binding)?;
    let result = named_pane(
        model::to_value(&AvatarAssignResult {
            binding: binding.clone(),
            pane_id: None,
        })?,
        pane_id,
    );
    let reply = core
        .mutate(connection, method::AVATAR_ASSIGN, params, |tx| {
            library::bind_in(tx, &binding)?;
            Ok((result, vec![event]))
        })?
        .result;
    rename(core, connection, pane_id, &name);
    Ok(Outcome::Reply(reply))
}

/// Sets the model or effort the pane's session announces, when it differs from the one it has.
fn set_option(
    core: &mut Core,
    connection: &str,
    pane_id: &str,
    key: &str,
    wanted: Option<&str>,
) -> Result<()> {
    let state = core.agent_state(pane_id)?;
    let Some((wanted, option)) = wanted.zip(requested_option(&state.options, key)) else {
        return Ok(());
    };
    if option.current_value == ConfigValue::V0(wanted.to_owned()) {
        return Ok(());
    }
    let set = PaneConfigSetParams {
        pane_id: pane_id.to_owned(),
        config_id: option.id.clone(),
        value: ConfigValue::V0(wanted.to_owned()),
        idempotency_key: Uuid::new_v4().to_string(),
    };
    core.agent_method(
        connection,
        method::PANE_CONFIG_SET,
        &model::to_value(&set)?,
        Vec::new(),
    )?;
    Ok(())
}

/// Marks the open pane as owing its persona to its next prompt, when its harness has no system
/// prompt to carry it.
fn owe_persona(core: &mut Core, pane_id: &str) -> Result<()> {
    let mut state = core.agent_state(pane_id)?;
    crate::agents::persona::mark_pending(&mut state);
    if !state.persona_pending {
        return Ok(());
    }
    let saved = core.pane(pane_id)?.stored.clone();
    core.commit_agent(saved, state, Changes::default(), None, json!({}))?;
    Ok(())
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
    let Ok(bindings) = library::bindings(&core.store) else {
        return;
    };
    for binding in bindings.iter().filter(|b| b.avatar_id == avatar.avatar_id) {
        let panes: Vec<String> = open_panes(core, &binding.workspace_id).cloned().collect();
        for pane_id in panes {
            rename(core, connection, &pane_id, &avatar.name);
        }
    }
}

/// What the person chose for the avatar's pane when they switched its harness.
pub(crate) struct Choice {
    harness: String,
    model: Option<String>,
    effort: Option<String>,
}

impl Choice {
    /// The harness, model and effort of a pane's new session, when the pane is an avatar's.
    pub(crate) fn of(state: &State) -> Option<Self> {
        let option = |key: &str| match state.requested_options.get(key) {
            Some(ConfigValue::V0(value)) => Some(value.clone()),
            _ => None,
        };
        state.avatar.then(|| Self {
            harness: state.launch.id.clone(),
            model: option("model"),
            effort: option("effort"),
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
