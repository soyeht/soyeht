//! The OpenCode meter through the running engine: the fixture machine of the test engine
//! (`usage-readers/`, ADR 0091) stands in for the home directory and for `curl`, so no
//! credential, network or OpenCode of the real machine is touched.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use serde_json::{Value, json};
use std::fs;
use support::{Rig, write_script};

const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const NO_PLAN: &str = r#"{"code":500,"msg":"当前用户不存在coding plan","success":false}"#;
const QUOTA: &str = r#"{"code":200,"success":true,"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":37}]}}"#;

/// A machine whose OpenCode is set to the GLM Coding Plan and signed in to it, with an `http-source`
/// that answers the quota request with `body`.
fn install_machine(rig: &Rig, body: &str) {
    let readers = rig.usage_readers();
    let home = readers.join("home");
    fs::create_dir_all(home.join(".config/opencode")).unwrap();
    fs::create_dir_all(home.join(".local/share/opencode")).unwrap();
    fs::write(
        home.join(".config/opencode/opencode.json"),
        r#"{"model":"zai-coding-plan/glm-5.3"}"#,
    )
    .unwrap();
    fs::write(
        home.join(".local/share/opencode/auth.json"),
        r#"{"zai-coding-plan":{"type":"api","key":"engine-fixture-token"}}"#,
    )
    .unwrap();
    write_script(
        &readers.join("http-source"),
        &format!(
            "#!/bin/sh\ncat >/dev/null\ncase \"$*\" in\n*{QUOTA_URL}*)\ncat <<'BODY'\n{body}\nBODY\necho SOYEHT_STATUS:200;;\n*) echo '{{}}'; echo SOYEHT_STATUS:404;;\nesac\necho SOYEHT_RETRY:\n"
        ),
    );
}

fn opencode(reply: &Value) -> &Value {
    reply["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reading| reading["provider"] == "opencode")
        .unwrap_or_else(|| panic!("no opencode reading in {reply}"))
}

#[test]
fn an_expired_glm_plan_reaches_the_wire_as_plan_expired() {
    let rig = Rig::new();
    install_machine(&rig, NO_PLAN);
    let mut client = rig.client();
    let listed = client.call("usage.list", json!({}));
    assert_eq!(opencode(&listed)["state"], "unavailable");
    assert_eq!(opencode(&listed)["unavailableReason"], "not_checked");

    let refreshed = client.call(
        "usage.refresh",
        json!({"provider":"opencode","idempotencyKey":"opencode-expired"}),
    );
    let reading = opencode(&refreshed);
    assert_eq!(reading["state"], "unavailable");
    assert_eq!(reading["unavailableReason"], "plan_expired");
    assert_eq!(reading["source"], "opencode_zai_plan");
    assert!(reading.get("primary").is_none() && reading.get("activity").is_none());
}

#[test]
fn a_glm_plan_quota_reaches_the_wire_as_a_window_and_survives_a_restart_as_last_known() {
    let mut rig = Rig::new();
    install_machine(&rig, QUOTA);
    let mut client = rig.client();
    let refreshed = client.call(
        "usage.refresh",
        json!({"provider":"opencode","idempotencyKey":"opencode-quota"}),
    );
    let reading = opencode(&refreshed);
    assert_eq!(reading["state"], "ok");
    assert_eq!(reading["source"], "opencode_zai_plan");
    assert_eq!(reading["primary"]["label"], "5-hour");
    assert_eq!(reading["primary"]["used"], 0.37);

    rig.kill_engine();
    rig.start();
    let mut client = rig.client();
    let listed = client.call("usage.list", json!({}));
    let reading = opencode(&listed);
    assert_eq!(reading["state"], "stale");
    assert_eq!(reading["primary"]["used"], 0.37);
}
