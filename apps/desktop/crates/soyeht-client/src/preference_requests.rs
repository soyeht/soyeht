//! The requests of Preferences and the harness switch: the harness list, the models a harness
//! offers, the settings that choose defaults, sign-in and sign-out, usage and skills. The
//! application asks for them as [`Command`]s; each is one typed request of the protocol.

use crate::{
    Result,
    catalogue::Catalogue,
    commands::{Command, key},
};
use serde_json::Value;
use soyeht_protocol::{
    EngineReadinessParams, HarnessListParams, ModelListParams, ModelListResult,
    PaneAuthLogoutParams, Request, SettingsGetParams, SettingsSetParams, SkillsCreateParams,
    SkillsDeleteParams, SkillsListParams, UsageRefreshParams, client_policy, setting,
};

/// A model list page; the next one is `cursor`.
pub fn catalogue_request(harness: &str, cursor: Option<String>) -> Request {
    Request::ModelList(ModelListParams {
        harness: harness.to_owned(),
        pane_id: None,
        cursor,
        limit: Some(client_policy::PAGE_SIZE_MODELS),
    })
}

pub fn setting_request(key: &str) -> Request {
    Request::SettingsGet(SettingsGetParams {
        key: key.to_owned(),
    })
}

fn set_request(name: String, value: &str) -> Request {
    Request::SettingsSet(SettingsSetParams {
        key: name,
        value: serde_json::Value::String(value.to_owned()),
        idempotency_key: key(),
    })
}

/// The request a command of this kind needs, or `None` when it is not one of these.
pub(crate) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::RefreshUsage { provider } => Request::UsageRefresh(UsageRefreshParams {
            provider: provider.clone(),
            idempotency_key: key(),
        }),
        Command::LoadHarnesses { rescan } => Request::HarnessList(HarnessListParams {
            cursor: None,
            limit: None,
            rescan: rescan.then_some(true),
        }),
        Command::LoadCatalogue { harness } => catalogue_request(harness, None),
        Command::LoadReadiness => Request::EngineReadiness(EngineReadinessParams {}),
        Command::LoadSkills => Request::SkillsList(SkillsListParams { workspace_id: None }),
        Command::CreateSkill {
            name,
            description,
            instructions,
        } => Request::SkillsCreate(SkillsCreateParams {
            name: name.clone(),
            description: description.clone(),
            instructions: Some(instructions.clone()).filter(|text| !text.trim().is_empty()),
            workspace_id: None,
            idempotency_key: key(),
        }),
        Command::DeleteSkill { name } => Request::SkillsDelete(SkillsDeleteParams {
            name: name.clone(),
            workspace_id: None,
            idempotency_key: key(),
        }),
        Command::SetPreferredHarness { harness } => {
            set_request(setting::HARNESS_PREFERRED_ID.to_owned(), harness)
        }
        Command::SetDefaultMode { harness, mode } => set_request(
            setting::harness_default_mode_key(harness),
            mode.as_deref().unwrap_or_default(),
        ),
        Command::SignOut { pane_id } => Request::PaneAuthLogout(PaneAuthLogoutParams {
            pane_id: pane_id.clone(),
            idempotency_key: key(),
        }),
        _ => return None,
    })
}

/// One page of a harness's model list, and where the next one starts.
pub(crate) fn catalogue_page(result: Value) -> Result<(Catalogue, Option<String>)> {
    let page = serde_json::from_value::<ModelListResult>(result)?;
    Ok((
        Catalogue {
            models: page.items,
            options: page.options,
            source: page.source,
        },
        page.next_cursor,
    ))
}
