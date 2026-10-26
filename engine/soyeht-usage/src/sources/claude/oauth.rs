//! Claude Code's OAuth login sent to Anthropic's usage endpoint
//! (CodexBar `ClaudeOAuthUsageFetcher`, `ClaudeOAuthCredentials`).

use super::super::{
    Attempt, Failure, ReadContext, Source, env_dir, http_body, json, read_json_file,
};
use crate::host::{Host, HttpRequest, Platform, Secret, SecretRead};
use serde_json::Value;
use soyeht_protocol::UsageSource;
use std::{path::PathBuf, time::Duration};

pub(crate) const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub(crate) const BETA: &str = "oauth-2025-04-20";
/// CodexBar sends the installed CLI's version and falls back to this one.
pub(crate) const USER_AGENT: &str = "claude-code/2.1.0";
pub(crate) const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const REQUIRED_SCOPE: &str = "user:profile";
const TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct OAuthSource;

impl Source for OAuthSource {
    fn id(&self) -> UsageSource {
        UsageSource::ClaudeOauthEndpoint
    }

    fn read(&self, host: &dyn Host, context: ReadContext) -> Attempt {
        let token = credential(host, &context)?;
        let request = HttpRequest::get(USAGE_URL, TIMEOUT)
            .bearer(&token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("anthropic-beta", BETA)
            .header("User-Agent", USER_AGENT);
        let body = http_body(host.http(&request))?;
        super::parse_windows(&json(&body)?)
    }
}

/// The access token: Claude Code's credentials file first, then, on macOS, its keychain item.
/// An expired or under-scoped login is "sign in needed": Soyeht never refreshes it.
fn credential(host: &dyn Host, context: &ReadContext) -> Result<Secret, Failure> {
    let now_sec = context.now_sec;
    let from_file = credentials_path(host)
        .ok_or(Failure::NotConfigured)
        .and_then(|path| read_json_file(host, &path))
        .and_then(|document| usable_token(&document, now_sec));
    match from_file {
        Ok(token) => Ok(token),
        Err(file_failure) if host.platform() == Platform::MacOs => {
            match context.keychain_password(host, KEYCHAIN_SERVICE) {
                SecretRead::Found(secret) => json(secret.expose().as_bytes())
                    .and_then(|document| usable_token(&document, now_sec)),
                SecretRead::Denied => Err(Failure::NoAccess),
                SecretRead::Missing | SecretRead::Unsupported => Err(file_failure),
            }
        }
        Err(failure) => Err(failure),
    }
}

/// Whether Claude Code keeps a login on this machine: a stored access or refresh token, in its
/// credentials file or, on macOS, its keychain item. The login is only looked for, never used.
///
/// A source that starts the Claude Code program asks this first. Without a login the program
/// would offer its sign-in menu, and a background probe must never choose from it (that opens
/// the browser). Expired is still a login: Claude Code refreshes it itself.
pub(super) fn stored_login(host: &dyn Host, context: &ReadContext) -> Result<(), Failure> {
    let file = credentials_path(host)
        .ok_or(Failure::NotConfigured)
        .and_then(|path| read_json_file(host, &path));
    // A file the engine cannot read is still a file Claude Code may read.
    if file
        .as_ref()
        .map_or_else(|failure| *failure == Failure::NoAccess, holds_login)
    {
        return Ok(());
    }
    if host.platform() != Platform::MacOs {
        return Err(Failure::LoginNeeded);
    }
    match context.keychain_password(host, KEYCHAIN_SERVICE) {
        SecretRead::Found(secret)
            if json(secret.expose().as_bytes()).is_ok_and(|document| holds_login(&document)) =>
        {
            Ok(())
        }
        SecretRead::Denied => Err(Failure::NoAccess),
        SecretRead::Found(_) | SecretRead::Missing | SecretRead::Unsupported => {
            Err(Failure::LoginNeeded)
        }
    }
}

fn holds_login(document: &Value) -> bool {
    let token = |key: &str| {
        document["claudeAiOauth"][key]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
    };
    token("accessToken") || token("refreshToken")
}

/// `$CLAUDE_SECURESTORAGE_CONFIG_DIR`, else the configuration root, plus `.credentials.json`.
fn credentials_path(host: &dyn Host) -> Option<PathBuf> {
    env_dir(host, "CLAUDE_SECURESTORAGE_CONFIG_DIR")
        .or_else(|| super::config_root(host))
        .map(|root| root.join(".credentials.json"))
}

fn usable_token(document: &Value, now_sec: u64) -> Result<Secret, Failure> {
    let oauth = document
        .get("claudeAiOauth")
        .ok_or(Failure::NotConfigured)?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(Secret::new)
        .filter(Secret::is_usable)
        .ok_or(Failure::NotConfigured)?;
    let expires_ms = oauth.get("expiresAt").and_then(Value::as_f64);
    if expires_ms.is_none_or(|expires| now_sec as f64 * 1000.0 >= expires) {
        return Err(Failure::LoginNeeded);
    }
    let scoped = oauth
        .get("scopes")
        .and_then(Value::as_array)
        .is_some_and(|scopes| scopes.iter().any(|scope| scope == REQUIRED_SCOPE));
    if !scoped {
        return Err(Failure::NoAccess);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expired_unscoped_and_absent_logins_are_distinct() {
        let login = |expires: u64, scopes: Value| json!({"claudeAiOauth": {"accessToken": "tok", "expiresAt": expires, "scopes": scopes}});
        assert!(usable_token(&login(2_000_000, json!(["user:profile"])), 1_000).is_ok());
        assert_eq!(
            usable_token(&login(500_000, json!(["user:profile"])), 1_000),
            Err(Failure::LoginNeeded)
        );
        assert_eq!(
            usable_token(&login(2_000_000, json!(["user:inference"])), 1_000),
            Err(Failure::NoAccess)
        );
        assert_eq!(
            usable_token(&json!({"mcpOAuth": {}}), 1_000),
            Err(Failure::NotConfigured)
        );
    }
}
