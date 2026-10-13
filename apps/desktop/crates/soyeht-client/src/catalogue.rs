//! What the engine lists for a harness before a pane exists: its models, and the options it
//! offers (effort, permission mode). The switch popover and Preferences choose from it.

use crate::{Error, Result, preference_requests, rpc::Rpc};
use soyeht_protocol::{ConfigOption, ModelInfo, ModelListSource};

/// The most pages of models read for one harness; a catalogue that keeps going is cut off.
const PAGES: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct Catalogue {
    pub models: Vec<ModelInfo>,
    pub options: Vec<ConfigOption>,
    pub source: ModelListSource,
}

impl Catalogue {
    /// The option that sets how hard the agent thinks: the harness's `thought_level`, or one
    /// named `effort`.
    pub fn effort_option(&self) -> Option<&ConfigOption> {
        effort_of(&self.options)
    }
}

/// Reads every page of a harness's model list.
pub(crate) fn load(rpc: &Rpc, harness: &str) -> Result<Catalogue> {
    let mut cursor = None;
    let mut all: Option<Catalogue> = None;
    for _ in 0..PAGES {
        let (page, next) = preference_requests::catalogue_page(rpc.call(
            &preference_requests::catalogue_request(harness, cursor.take()),
        )?)?;
        match all.as_mut() {
            Some(known) => {
                known.models.extend(page.models);
                known.options = page.options;
                known.source = page.source;
            }
            None => all = Some(page),
        }
        match next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    all.ok_or(Error::Invalid("the engine listed no page of models"))
}

/// The effort option among `options`.
pub fn effort_of(options: &[ConfigOption]) -> Option<&ConfigOption> {
    options.iter().find(|option| option.is_effort())
}

/// What the model list of a harness is in, for what the switch popover shows under "MODEL".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelState {
    Loading,
    Failed,
    /// The agent has not announced its models; they appear after it starts.
    Unavailable,
    /// It announced none.
    Empty,
    Results,
}

/// The state of the model list: still loading while nothing has arrived, failed when the last
/// request did and nothing is held, else what is held.
pub fn model_state(catalogue: Option<&Catalogue>, loading: bool, failed: bool) -> ModelState {
    match catalogue {
        None if loading => ModelState::Loading,
        None if failed => ModelState::Failed,
        None => ModelState::Unavailable,
        Some(catalogue) if catalogue.models.is_empty() => ModelState::Empty,
        Some(_) => ModelState::Results,
    }
}
