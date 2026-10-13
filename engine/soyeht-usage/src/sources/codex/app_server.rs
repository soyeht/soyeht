//! A disposable `codex app-server` asked for `account/rateLimits/read`
//! (CodexBar `CodexCLIUsageStrategy`, `UsageFetcher.loadLatestCLIAccountSnapshot`).
//!
//! The rate limits are read only after `account/read` with `refreshToken`: this source is
//! reached when the login file holds a token the usage endpoint refuses or is about to refuse,
//! and the app server refreshes that token itself only when asked. Reading the limits first
//! gets "token expired" and the child is gone before it refreshes anything.

use super::super::{Attempt, Failure, ReadContext, Source};
use crate::host::{Host, RpcCall, RpcRequest, RpcRun};
use serde_json::json;
use soyeht_protocol::UsageSource;
use std::time::Duration;

pub(crate) const PROGRAM: &str = "codex";
pub(crate) const ARGS: [&str; 5] = ["-s", "read-only", "-a", "never", "app-server"];
const TIMEOUT: Duration = Duration::from_secs(15);
/// CodexBar pauses background app-server launches for 30 minutes after one fails to start.
const LAUNCH_FAILURE_COOLDOWN_SEC: u64 = 30 * 60;

pub(crate) struct AppServerSource;

impl Source for AppServerSource {
    fn id(&self) -> UsageSource {
        UsageSource::CodexAppServer
    }

    fn automatic_cooldown_sec(&self, failure: &Failure) -> Option<u64> {
        match failure {
            Failure::ToolNotFound | Failure::Failed => Some(LAUNCH_FAILURE_COOLDOWN_SEC),
            other => super::super::default_cooldown_sec(other),
        }
    }

    fn read(&self, host: &dyn Host, _context: ReadContext) -> Attempt {
        let request = RpcRequest {
            program: PROGRAM.into(),
            args: ARGS.iter().map(|arg| (*arg).to_owned()).collect(),
            calls: vec![
                RpcCall {
                    method: "initialize".into(),
                    params: json!({"clientInfo": {"name": "soyeht", "version": env!("CARGO_PKG_VERSION")}}),
                    id: Some(1),
                },
                RpcCall {
                    method: "initialized".into(),
                    params: json!({}),
                    id: None,
                },
                RpcCall {
                    method: "account/read".into(),
                    params: json!({"refreshToken": true}),
                    id: Some(2),
                },
                RpcCall {
                    method: "account/rateLimits/read".into(),
                    params: json!({}),
                    id: Some(3),
                },
            ],
            timeout: TIMEOUT,
        };
        match host.json_rpc(&request) {
            // The last result is the rate limits; a refusal arrives as `null`. The refresh
            // before it may be refused too (no login to refresh), which is not the end of the read.
            RpcRun::Results(results) => results
                .last()
                .filter(|limits| !limits.is_null())
                .ok_or(Failure::Failed)
                .and_then(super::parse_rate_limits),
            RpcRun::ToolNotFound => Err(Failure::ToolNotFound),
            RpcRun::Failed => Err(Failure::Failed),
        }
    }
}
