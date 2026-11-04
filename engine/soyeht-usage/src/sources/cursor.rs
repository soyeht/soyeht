//! Cursor: the Cursor app's own login, then the cursor.com session from the user's browsers,
//! both sent to cursor.com's usage summary (CodexBar `CursorStatusProbe`, `CursorAppAuth`).
//! Browsers are read on macOS only, as in CodexBar; Linux uses the app login.

use super::{
    Attempt, Failure, ParsedReading, ReadContext, Source, env_dir, first_browser_session,
    http_body, iso_text, json, jwt, number, percent_window,
};
use crate::host::{CookieJar, Host, HttpRequest, Platform, Secret, SecretRead};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::{path::PathBuf, time::Duration};

pub(crate) const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";
pub(crate) const ACCESS_TOKEN_KEY: &str = "cursorAuth/accessToken";
const TIMEOUT: Duration = Duration::from_secs(15);
/// CodexBar uses the app token only while it has more than a minute left.
const EXPIRY_MARGIN_SEC: i64 = 60;
const SESSION_COOKIES: [&str; 7] = [
    "WorkosCursorSessionToken",
    "__Secure-next-auth.session-token",
    "next-auth.session-token",
    "wos-session",
    "__Secure-wos-session",
    "authjs.session-token",
    "__Secure-authjs.session-token",
];
const COOKIE_DOMAINS: [&str; 2] = ["cursor.com", "cursor.sh"];

pub(crate) fn chain() -> Vec<Box<dyn Source>> {
    vec![Box::new(AppLoginSource), Box::new(BrowserSource)]
}

/// A signed-out or rejected session moves on to the next one; anything else is the answer.
fn session_problem(failure: &Failure) -> bool {
    matches!(
        failure,
        Failure::NotConfigured | Failure::LoginNeeded | Failure::NoAccess
    )
}

pub(crate) struct AppLoginSource;

impl Source for AppLoginSource {
    fn id(&self) -> UsageSource {
        UsageSource::CursorAppLogin
    }

    fn falls_back(&self, failure: &Failure) -> bool {
        session_problem(failure)
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        let path = state_database(host).ok_or(Failure::NotConfigured)?;
        let token = match host.state_db_value(&path, ACCESS_TOKEN_KEY) {
            SecretRead::Found(token) => token,
            SecretRead::Denied => return Err(Failure::NoAccess),
            SecretRead::Missing | SecretRead::Unsupported => return Err(Failure::NotConfigured),
        };
        let cookie = app_cookie(&token, context.now_sec)?;
        summary(host, cookie)
    }
}

pub(crate) struct BrowserSource;

impl Source for BrowserSource {
    fn id(&self) -> UsageSource {
        UsageSource::CursorBrowserCookies
    }

    fn read(&self, host: &dyn Host, _context: ReadContext) -> Attempt {
        let session = |jar: &CookieJar| {
            SESSION_COOKIES
                .iter()
                .any(|name| jar.get(name).is_some())
                .then(|| jar.header())
        };
        first_browser_session(host, &COOKIE_DOMAINS, session, |cookie| {
            summary(host, cookie)
        })
    }
}

/// `~/Library/Application Support/Cursor/...` on macOS; `$XDG_CONFIG_HOME/Cursor/...`, else
/// `~/.config/Cursor/...`, on Linux; `%APPDATA%\Cursor\...` on Windows.
fn state_database(host: &dyn Host) -> Option<PathBuf> {
    let root = match host.platform() {
        Platform::MacOs => host.home()?.join("Library/Application Support"),
        Platform::Windows => env_dir(host, "APPDATA")
            .filter(|path| path.is_absolute())
            .or_else(|| host.home().map(|home| home.join("AppData/Roaming")))?,
        Platform::Linux => env_dir(host, "XDG_CONFIG_HOME")
            .filter(|path| path.is_absolute())
            .or_else(|| host.home().map(|home| home.join(".config")))?,
    };
    Some(root.join("Cursor/User/globalStorage/state.vscdb"))
}

/// `WorkosCursorSessionToken=<user id>%3A%3A<token>`, the cookie cursor.com sets after login.
fn app_cookie(token: &Secret, now_sec: u64) -> Result<Secret, Failure> {
    let claims = jwt::claims(token.expose()).ok_or(Failure::NotConfigured)?;
    let user = claims["sub"]
        .as_str()
        .and_then(|subject| subject.rsplit('|').next())
        .filter(|user| {
            !user.is_empty()
                && user
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .ok_or(Failure::NotConfigured)?;
    let expiry = claims["exp"].as_i64().ok_or(Failure::NotConfigured)?;
    if expiry - i64::try_from(now_sec).unwrap_or(i64::MAX) <= EXPIRY_MARGIN_SEC {
        return Err(Failure::LoginNeeded);
    }
    if !token.is_usable() {
        return Err(Failure::NotConfigured);
    }
    Ok(Secret::new(format!(
        "WorkosCursorSessionToken={user}%3A%3A{}",
        token.expose()
    )))
}

fn summary(host: &dyn Host, cookie: Secret) -> Attempt {
    let request = HttpRequest::get(SUMMARY_URL, TIMEOUT)
        .header("Accept", "application/json")
        .secret_header("Cookie", cookie);
    parse_summary(&json(&http_body(host.http(&request))?)?)
}

/// The headline percent, by CodexBar's precedence: `totalPercentUsed`, the mean of the auto
/// and API percentages, either one, then plan, overall and pooled used/limit in cents.
fn parse_summary(document: &Value) -> Result<ParsedReading, Failure> {
    if !document.is_object() {
        return Err(Failure::InvalidResponse);
    }
    let individual = &document["individualUsage"];
    let plan = &individual["plan"];
    let ratio = |block: &Value| {
        let used = number(&block["used"])?;
        let limit = number(&block["limit"]).filter(|limit| *limit > 0.0)?;
        Some(used / limit * 100.0)
    };
    let auto = number(&plan["autoPercentUsed"]);
    let api = number(&plan["apiPercentUsed"]);
    let percent = number(&plan["totalPercentUsed"])
        .or(match (auto, api) {
            (Some(auto), Some(api)) => Some((auto + api) / 2.0),
            (auto, api) => api.or(auto),
        })
        .or_else(|| ratio(plan))
        .or_else(|| ratio(&individual["overall"]))
        .or_else(|| ratio(&document["teamUsage"]["pooled"]))
        .unwrap_or(0.0);
    let resets_at = document["billingCycleEnd"].as_str().and_then(iso_text);
    Ok(ParsedReading::windows(
        percent_window(percent, resets_at, "Billing cycle")?,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::json;

    #[test]
    fn app_token_becomes_the_session_cookie() {
        let claims = URL_SAFE_NO_PAD.encode(br#"{"sub":"auth0|user_01","exp":2000000000}"#);
        let token = Secret::new(format!("h.{claims}.s"));
        let cookie = app_cookie(&token, 1_000).unwrap();
        assert_eq!(
            cookie.expose(),
            format!("WorkosCursorSessionToken=user_01%3A%3Ah.{claims}.s")
        );
        assert_eq!(
            app_cookie(&token, 2_000_000_000).err(),
            Some(Failure::LoginNeeded)
        );
    }

    #[test]
    fn headline_percent_follows_the_precedence() {
        let total = json!({"billingCycleEnd": "2026-10-20T00:00:00.000Z",
            "individualUsage": {"plan": {"totalPercentUsed": 36.5, "autoPercentUsed": 10, "apiPercentUsed": 90}}});
        let reading = parse_summary(&total).unwrap();
        assert_eq!(reading.primary.clone().unwrap().used, 0.365);
        assert_eq!(
            reading.primary.clone().unwrap().resets_at,
            "2026-10-20T00:00:00Z"
        );
        let mean =
            json!({"individualUsage": {"plan": {"autoPercentUsed": 10, "apiPercentUsed": 30}}});
        assert!((parse_summary(&mean).unwrap().primary.clone().unwrap().used - 0.2).abs() < 1e-9);
        let cents = json!({"individualUsage": {"plan": {"used": 500, "limit": 2000}}});
        assert_eq!(
            parse_summary(&cents).unwrap().primary.clone().unwrap().used,
            0.25
        );
        let pooled = json!({"teamUsage": {"pooled": {"used": 1, "limit": 4}}});
        assert_eq!(
            parse_summary(&pooled)
                .unwrap()
                .primary
                .clone()
                .unwrap()
                .used,
            0.25
        );
    }
}
