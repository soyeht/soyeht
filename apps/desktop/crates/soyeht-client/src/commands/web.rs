//! The requests of a web pane: opening one, sending it somewhere, and telling the engine where
//! the page is.

use super::{Command, key};
use soyeht_protocol::{
    PaneCreateParams, PaneMode, Request, WebHostNavigatedParams, WebNavigateParams,
};

pub(super) fn request(command: &Command) -> Option<Request> {
    Some(match command {
        Command::OpenWeb {
            workspace_id,
            url,
            incognito,
            ..
        } => Request::PaneCreate(PaneCreateParams {
            workspace_id: workspace_id.clone(),
            mode: PaneMode::Web,
            cwd: None,
            name: None,
            cols: None,
            rows: None,
            idempotency_key: key(),
            harness: None,
            model: None,
            effort: None,
            permission_mode: None,
            url: url.clone(),
            incognito: incognito.then_some(true),
            path: None,
            writing_kind: None,
            writing_section: None,
        }),
        Command::NavigateWeb {
            pane_id,
            url,
            action,
        } => Request::WebNavigate(WebNavigateParams {
            pane_id: pane_id.clone(),
            url: url.clone(),
            action: action.clone(),
            idempotency_key: key(),
        }),
        Command::ReportWebPage {
            pane_id,
            generation,
            url,
            title,
        } => Request::WebHostNavigated(WebHostNavigatedParams {
            pane_id: pane_id.clone(),
            navigation_generation: generation.to_string(),
            url: url.clone(),
            title: title.clone(),
            idempotency_key: key(),
        }),
        _ => return None,
    })
}
