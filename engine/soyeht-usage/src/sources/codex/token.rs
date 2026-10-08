//! The two `auth.json` logins Codex keeps, sent to the ChatGPT usage endpoint
//! (CodexBar `CodexPATFetchStrategy`, `CodexOAuthFetchStrategy`, `CodexOAuthUsageFetcher`).
//!
//! Neither source refreshes a token. A login that is about to expire falls through to the app
//! server, where Codex refreshes its own login (CodexBar does the same).

use super::super::{Attempt, Failure, ReadContext, Source, http_body, json, jwt, read_json_file};
use crate::host::{FileRead, Host, HttpRequest, Secret};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
pub(crate) const WHOAMI_URL: &str =
    "https://auth.openai.com/api/accounts/v1/user-auth-credential/whoami";
const TIMEOUT: Duration = Duration::from_secs(30);
/// CodexBar treats a Codex login that expires within five minutes as one Codex must refresh.
const REFRESH_MARGIN_SEC: i64 = 5 * 60;
const CONFIG_MAX_BYTES: u64 = 256 * 1024;
pub(crate) const PAT_USER_AGENT: &str = "codex_cli_rs";
const ACCOUNT_USER_AGENT: &str = "Soyeht";

/// Both sources fall through only when the login itself is missing or refused, never on a
/// network or server error, so a flaky API does not start an app server every time.
fn login_problem(failure: &Failure) -> bool {
    matches!(
        failure,
        Failure::NotConfigured | Failure::LoginNeeded | Failure::NoAccess
    )
}

/// `personal_access_token` in `auth.json`: identity from the whoami call, then usage.
pub(crate) struct AccessTokenSource;

impl Source for AccessTokenSource {
    fn id(&self) -> UsageSource {
        UsageSource::CodexAccessToken
    }

    fn falls_back(&self, failure: &Failure) -> bool {
        login_problem(failure)
    }

    fn read(&self, host: &dyn Host, _context: ReadContext) -> Attempt {
        let auth = auth_document(host)?;
        let token = ["personal_access_token", "personalAccessToken"]
            .iter()
            .find_map(|key| auth.get(*key).and_then(Value::as_str))
            .map(|value| Secret::new(value.trim()))
            .filter(Secret::is_usable)
            .ok_or(Failure::NotConfigured)?;
        let whoami = json(&http_body(
            host.http(&pat_headers(HttpRequest::get(WHOAMI_URL, TIMEOUT), &token)),
        )?)?;
        let account = whoami["chatgpt_account_id"].as_str().and_then(account_id);
        let request = pat_headers(HttpRequest::get(usage_url(host), TIMEOUT), &token);
        read_usage(host, request, account)
    }
}

fn pat_headers(request: HttpRequest, token: &Secret) -> HttpRequest {
    request
        .bearer(token)
        .header("User-Agent", PAT_USER_AGENT)
        .header("Accept", "application/json")
        .header("originator", PAT_USER_AGENT)
}

/// The usage call itself, naming the account when the login has one.
fn read_usage(host: &dyn Host, mut request: HttpRequest, account: Option<String>) -> Attempt {
    if let Some(account) = account {
        request = request.header("ChatGPT-Account-Id", &account);
    }
    super::parse_usage(&json(&http_body(host.http(&request))?)?)
}

/// The ChatGPT login (`tokens.access_token`) or an `OPENAI_API_KEY` stored by `codex login`.
pub(crate) struct AccountSource;

impl Source for AccountSource {
    fn id(&self) -> UsageSource {
        UsageSource::CodexAccountEndpoint
    }

    fn falls_back(&self, failure: &Failure) -> bool {
        login_problem(failure)
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        let auth = auth_document(host)?;
        let (token, account) = account_login(&auth, context.now_sec)?;
        let request = HttpRequest::get(usage_url(host), TIMEOUT)
            .bearer(&token)
            .header("User-Agent", ACCOUNT_USER_AGENT)
            .header("Accept", "application/json");
        read_usage(host, request, account)
    }
}

fn auth_document(host: &dyn Host) -> Result<Value, Failure> {
    let path = super::codex_home(host)
        .ok_or(Failure::NotConfigured)?
        .join("auth.json");
    read_json_file(host, &path)
}

fn account_login(auth: &Value, now_sec: u64) -> Result<(Secret, Option<String>), Failure> {
    if let Some(key) = auth["OPENAI_API_KEY"]
        .as_str()
        .map(|key| Secret::new(key.trim()))
        .filter(Secret::is_usable)
    {
        return Ok((key, None));
    }
    let tokens = auth.get("tokens").ok_or(Failure::NotConfigured)?;
    let field = |snake: &str, camel: &str| {
        tokens
            .get(snake)
            .or_else(|| tokens.get(camel))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let access = field("access_token", "accessToken")
        .map(Secret::new)
        .filter(Secret::is_usable)
        .ok_or(Failure::NotConfigured)?;
    field("refresh_token", "refreshToken").ok_or(Failure::NotConfigured)?;
    if let Some(expiry) = jwt::expiry(access.expose()) {
        let now = i64::try_from(now_sec).unwrap_or(i64::MAX);
        if expiry - now <= REFRESH_MARGIN_SEC {
            return Err(Failure::LoginNeeded);
        }
    }
    let account = field("account_id", "accountId")
        .and_then(|value| account_id(&value))
        .or_else(|| {
            [
                field("id_token", "idToken"),
                Some(access.expose().to_owned()),
            ]
            .into_iter()
            .flatten()
            .find_map(|token| jwt_account(&token))
        });
    Ok((access, account))
}

fn jwt_account(token: &str) -> Option<String> {
    let claims = jwt::claims(token)?;
    [
        claims.get("chatgpt_account_id"),
        claims
            .get("https://api.openai.com/auth")
            .and_then(|auth| auth.get("chatgpt_account_id")),
        claims
            .get("organizations")
            .and_then(|organizations| organizations.get(0))
            .and_then(|organization| organization.get("id")),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| value.as_str().and_then(account_id))
}

/// An account id safe to send as a header.
fn account_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then(|| value.to_owned())
}

/// `chatgpt_base_url` from Codex's `config.toml`, else chatgpt.com; `/wham/usage` under a
/// `/backend-api` base and `/api/codex/usage` otherwise.
pub(crate) fn usage_url(host: &dyn Host) -> String {
    let configured = super::codex_home(host)
        .map(|home| home.join("config.toml"))
        .and_then(|path| match host.read_file(&path, CONFIG_MAX_BYTES) {
            FileRead::Bytes(bytes) => String::from_utf8(bytes).ok(),
            FileRead::Missing | FileRead::Unreadable => None,
        })
        .and_then(|text| base_url_setting(&text));
    let mut base = configured.unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    while base.ends_with('/') {
        base.pop();
    }
    if (base.starts_with("https://chatgpt.com") || base.starts_with("https://chat.openai.com"))
        && !base.contains("/backend-api")
    {
        base.push_str("/backend-api");
    }
    if base.contains("/backend-api") {
        format!("{base}/wham/usage")
    } else {
        format!("{base}/api/codex/usage")
    }
}

fn base_url_setting(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.split('#').next()?.trim();
        let (key, value) = line.split_once('=')?;
        (key.trim() == "chatgpt_base_url")
            .then(|| {
                value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_owned()
            })
            .filter(|value| value.starts_with("https://"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::json;

    fn token(claims: Value) -> String {
        format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims.to_string()))
    }

    #[test]
    fn account_comes_from_the_file_or_the_token_claims() {
        let access = token(
            json!({"exp": 2_000_000_000, "https://api.openai.com/auth": {"chatgpt_account_id": "acct-1"}}),
        );
        let auth = json!({"tokens": {"access_token": access, "refresh_token": "r"}});
        let (_, account) = account_login(&auth, 1_000).unwrap();
        assert_eq!(account.as_deref(), Some("acct-1"));
        let auth = json!({"tokens": {"access_token": access, "refresh_token": "r", "account_id": "acct-2"}});
        assert_eq!(
            account_login(&auth, 1_000).unwrap().1.as_deref(),
            Some("acct-2")
        );
    }

    #[test]
    fn a_login_codex_must_refresh_is_left_to_codex() {
        let access = token(json!({"exp": 1_000_100}));
        let auth = json!({"tokens": {"access_token": access, "refresh_token": "r"}});
        assert_eq!(
            account_login(&auth, 1_000_000).err(),
            Some(Failure::LoginNeeded)
        );
        assert_eq!(
            account_login(&json!({"tokens": {}}), 0).err(),
            Some(Failure::NotConfigured)
        );
    }

    #[test]
    fn configured_base_urls_choose_the_usage_path() {
        assert_eq!(
            base_url_setting("# x\nchatgpt_base_url = \"https://example.test/api\" # note\n")
                .as_deref(),
            Some("https://example.test/api")
        );
        assert_eq!(
            base_url_setting("chatgpt_base_url = \"http://plain\""),
            None
        );
    }
}
