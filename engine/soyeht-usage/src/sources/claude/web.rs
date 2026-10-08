//! The claude.ai session cookie from the user's browsers sent to claude.ai's own usage API
//! (CodexBar `ClaudeWebAPIFetcher`). macOS only, as in CodexBar.

use super::super::{Attempt, Failure, ReadContext, Source, first_browser_session, http_body, json};
use crate::host::{CookieJar, Host, HttpRequest, HttpResult, Secret};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::time::Duration;

pub(crate) const DOMAIN: &str = "claude.ai";
pub(crate) const ORGANIZATIONS_URL: &str = "https://claude.ai/api/organizations";
const SESSION_COOKIE: &str = "sessionKey";
const SESSION_PREFIX: &str = "sk-ant-";
const TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) struct WebSource;

impl Source for WebSource {
    fn id(&self) -> UsageSource {
        UsageSource::ClaudeWeb
    }

    fn read(&self, host: &dyn Host, _context: ReadContext) -> Attempt {
        first_browser_session(host, &[DOMAIN], session_key, |session| {
            read_session(host, session)
        })
    }
}

fn session_key(jar: &CookieJar) -> Option<Secret> {
    jar.get(SESSION_COOKIE)
        .filter(|value| value.expose().starts_with(SESSION_PREFIX) && value.is_usable())
        .cloned()
}

fn request(url: &str, session: &Secret) -> HttpRequest {
    HttpRequest::get(url, TIMEOUT)
        .secret_header(
            "Cookie",
            Secret::new(format!("{SESSION_COOKIE}={}", session.expose())),
        )
        .header("Accept", "application/json")
}

fn read_session(host: &dyn Host, session: Secret) -> Attempt {
    let organizations = json(&http_body(
        host.http(&request(ORGANIZATIONS_URL, &session)),
    )?)?;
    let organization = organization(&organizations).ok_or(Failure::InvalidResponse)?;
    let usage = format!("{ORGANIZATIONS_URL}/{organization}/usage");
    let first = host.http(&request(&format!("{usage}?cedar_ember=1"), &session));
    // CodexBar retries once without the query when the first answer is neither a success,
    // a rejected session nor a rate limit.
    let response = match first {
        HttpResult::Response { status, .. } if !matches!(status, 200 | 401 | 429) => {
            host.http(&request(&usage, &session))
        }
        other => other,
    };
    super::parse_windows(&json(&http_body(response)?)?)
}

/// The first organization with chat, else the first that is not API-only, else the first.
fn organization(document: &Value) -> Option<String> {
    let organizations = document.as_array()?;
    let capabilities = |organization: &Value| -> Vec<String> {
        organization["capabilities"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_ascii_lowercase)
                    .collect()
            })
            .unwrap_or_default()
    };
    let api_only = |organization: &Value| capabilities(organization) == ["api"];
    organizations
        .iter()
        .find(|organization| capabilities(organization).iter().any(|item| item == "chat"))
        .or_else(|| {
            organizations
                .iter()
                .find(|organization| !api_only(organization))
        })
        .or_else(|| organizations.first())
        .and_then(|organization| organization["uuid"].as_str())
        .filter(|uuid| {
            !uuid.is_empty()
                && uuid.len() <= 64
                && uuid
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_organization_wins_over_api_only() {
        let document = json!([
            {"uuid": "aaaa-0001", "capabilities": ["api"]},
            {"uuid": "bbbb-0002", "capabilities": ["chat", "claude_pro"]}
        ]);
        assert_eq!(organization(&document).as_deref(), Some("bbbb-0002"));
        let unsafe_id = json!([{"uuid": "../../x", "capabilities": ["chat"]}]);
        assert_eq!(organization(&unsafe_id), None);
    }
}
