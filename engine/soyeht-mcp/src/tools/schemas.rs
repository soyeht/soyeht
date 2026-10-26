//! The JSON schema of each tool's arguments, built from small field constructors.

use serde_json::{Map, Value, json};

pub(super) fn agent_message_schema() -> Value {
    object(
        fields(&[
            ("targetPaneId", id()),
            (
                "text",
                bounded_string(soyeht_protocol::limits::AGENT_MESSAGE_BYTES),
            ),
        ]),
        &["targetPaneId", "text"],
    )
}

pub(super) fn object(properties: Map<String, Value>, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn fields(entries: &[(&str, Value)]) -> Map<String, Value> {
    entries
        .iter()
        .map(|(name, value)| ((*name).into(), value.clone()))
        .collect()
}

pub(super) fn string() -> Value {
    json!({"type":"string"})
}
pub(super) fn id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":128})
}
pub(super) fn bounded_string(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
pub(super) fn boolean() -> Value {
    json!({"type":"boolean"})
}
pub(super) fn integer(minimum: i64, maximum: i64) -> Value {
    json!({"type":"integer","minimum":minimum,"maximum":maximum})
}
pub(super) fn decimal() -> Value {
    json!({"type":"string","pattern":"^(0|[1-9][0-9]*)$","maxLength":20})
}
pub(super) fn strings() -> Value {
    json!({"type":"array","items":{"type":"string"}})
}
pub(super) fn empty_schema() -> Value {
    object(Map::new(), &[])
}
pub(super) fn task_selector_schema() -> Value {
    object(fields(&[("taskId", id())]), &[])
}
pub(super) fn release_schema() -> Value {
    object(fields(&[("reason", string())]), &[])
}
pub(super) fn comment_schema() -> Value {
    object(fields(&[("taskId", id()), ("text", string())]), &["text"])
}
pub(super) fn subitem_add_schema() -> Value {
    object(
        fields(&[
            ("text", string()),
            ("agent", id()),
            ("instructions", string()),
        ]),
        &["text"],
    )
}
pub(super) fn subitem_done_schema() -> Value {
    object(fields(&[("subitemId", id())]), &["subitemId"])
}
pub(super) fn unblock_schema() -> Value {
    object(
        fields(&[("taskId", id()), ("reason", string())]),
        &["taskId"],
    )
}

pub(super) fn block_schema() -> Value {
    object(
        fields(&[
            (
                "type",
                json!({"type":"string","enum":["dependency","missing_information","external_error"]}),
            ),
            ("reason", string()),
            ("dependencyTaskId", id()),
        ]),
        &["type", "reason"],
    )
}

pub(super) fn create_schema() -> Value {
    object(
        fields(&[
            ("title", string()),
            ("description", string()),
            ("howToVerify", string()),
            ("mustNotChange", string()),
            (
                "executor",
                json!({"type":"object","required":["workspaceDefault"]}),
            ),
            (
                "priority",
                json!({"type":"string","enum":["urgent","high","medium","low"]}),
            ),
            ("createWorktree", json!({"type":"boolean"})),
            ("permissionMode", id()),
            ("dependencies", strings()),
            ("dispatch", json!({"type":"boolean"})),
        ]),
        &["title", "mustNotChange"],
    )
}

pub(super) fn list_schema() -> Value {
    object(
        fields(&[
            (
                "state",
                json!({"type":"string","enum":["todo","in_progress","in_review","completed","cancelled"]}),
            ),
            ("limit", json!({"type":"integer","minimum":1,"maximum":200})),
            ("cursor", id()),
        ]),
        &["limit"],
    )
}

pub(super) fn update_schema() -> Value {
    object(
        fields(&[
            ("taskId", id()),
            ("version", json!({"type":"integer","minimum":0})),
            ("title", string()),
            ("description", string()),
            (
                "priority",
                json!({"type":"string","enum":["urgent","high","medium","low"]}),
            ),
            ("dependencies", strings()),
        ]),
        &["version"],
    )
}

pub(super) fn complete_schema() -> Value {
    object(
        fields(&[
            ("summary", string()),
            (
                "contractChanges",
                json!({"type":"array","items":{"type":"object","required":["path","name","label","contractRef"]}}),
            ),
            (
                "outOfScopeEdits",
                json!({"type":"array","items":{"type":"object","required":["path","reason"]}}),
            ),
        ]),
        &["summary", "contractChanges", "outOfScopeEdits"],
    )
}

pub(super) fn dispatch_schema() -> Value {
    object(
        fields(&[
            ("taskId", id()),
            (
                "preset",
                json!({"type":"object","required":["workspaceDefault"]}),
            ),
        ]),
        &["taskId", "preset"],
    )
}

pub(super) fn loop_schema() -> Value {
    object(
        fields(&[
            ("prompt", string()),
            ("interval", json!({"type":"integer","minimum":0})),
            ("maxRounds", json!({"type":"integer","minimum":0})),
            (
                "stopWhen",
                json!({"type":"string","enum":["round_limit","three_empty","manual"]}),
            ),
            ("enabled", json!({"type":"boolean"})),
        ]),
        &["prompt", "interval", "maxRounds", "stopWhen", "enabled"],
    )
}

pub(super) fn writing_structure_schema() -> Value {
    object(
        fields(&[("fileId", id()), ("revision", decimal())]),
        &["fileId", "revision"],
    )
}

pub(super) fn writing_variant_add_schema() -> Value {
    object(
        fields(&[
            ("fileId", id()),
            ("lineId", id()),
            ("baseRevision", decimal()),
            (
                "text",
                bounded_string(soyeht_protocol::limits::WRITING_VARIANT_BYTES),
            ),
        ]),
        &["fileId", "lineId", "baseRevision", "text"],
    )
}

pub(super) fn web_open_schema() -> Value {
    object(
        fields(&[
            (
                "url",
                json!({"type":"string","minLength":1,"maxLength":soyeht_protocol::limits::WEB_FIELD_BYTES}),
            ),
            ("workspaceId", id()),
            ("newPane", boolean()),
            ("incognito", boolean()),
            ("mode", json!({"type":"string","enum":["structured"]})),
        ]),
        &["url"],
    )
}

pub(super) fn web_navigate_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "url",
                json!({"type":"string","minLength":1,"maxLength":soyeht_protocol::limits::WEB_FIELD_BYTES}),
            ),
            (
                "action",
                json!({"type":"string","enum":["back","forward","reload"]}),
            ),
        ]),
        &["paneId"],
    )
}

pub(super) fn web_pane_schema() -> Value {
    object(fields(&[("paneId", id())]), &["paneId"])
}

pub(super) fn web_snapshot_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("screenshot", boolean())]),
        &["paneId"],
    )
}

pub(super) fn web_ref_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("ref", id())]),
        &["paneId", "ref"],
    )
}

pub(super) fn web_type_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            ("text", bounded_string(65_536)),
            ("submit", boolean()),
        ]),
        &["paneId", "ref", "text"],
    )
}

pub(super) fn web_press_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "key",
                json!({"type":"string","minLength":1,"maxLength":128}),
            ),
        ]),
        &["paneId", "key"],
    )
}

pub(super) fn web_select_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            (
                "value",
                bounded_string(soyeht_protocol::limits::WEB_FIELD_BYTES),
            ),
        ]),
        &["paneId", "ref", "value"],
    )
}

pub(super) fn web_scroll_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            ("ref", id()),
            ("dx", integer(-1_000_000, 1_000_000)),
            ("dy", integer(-1_000_000, 1_000_000)),
        ]),
        &["paneId", "dx", "dy"],
    )
}

pub(super) fn web_wait_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "text",
                bounded_string(soyeht_protocol::limits::WEB_FIELD_BYTES),
            ),
            (
                "selector",
                bounded_string(soyeht_protocol::limits::WEB_FIELD_BYTES),
            ),
            ("networkIdle", boolean()),
            ("timeoutMs", integer(0, 60_000)),
        ]),
        &["paneId", "timeoutMs"],
    )
}

pub(super) fn web_evaluate_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("js", bounded_string(65_536))]),
        &["paneId", "js"],
    )
}

pub(super) fn web_screenshot_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("fullPage", boolean())]),
        &["paneId"],
    )
}

pub(super) fn web_read_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "screens",
                integer(1, soyeht_protocol::limits::WEB_READ_SCREENS as i64),
            ),
        ]),
        &["paneId"],
    )
}

pub(super) fn web_since_schema() -> Value {
    object(
        fields(&[("paneId", id()), ("since", decimal())]),
        &["paneId"],
    )
}

pub(super) fn web_resize_schema() -> Value {
    object(
        fields(&[
            ("paneId", id()),
            (
                "preset",
                json!({"type":"string","enum":["iphone_se","iphone_15_pro","ipad","responsive"]}),
            ),
            ("width", integer(1, 16_384)),
            ("height", integer(1, 16_384)),
            ("rotate", boolean()),
        ]),
        &["paneId"],
    )
}
