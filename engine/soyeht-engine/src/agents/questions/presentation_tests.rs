use super::*;

/// The public form of a permission request, as the engine presents it to a client.
fn permission(
    params: &Value,
    tracked_tool: Option<&Value>,
    pane_names: &PaneNames<'_>,
) -> Result<(Value, &'static str)> {
    public_question(
        "session/request_permission",
        params,
        tracked_tool,
        json!({}),
        pane_names,
    )
}

#[test]
fn permission_exposes_typed_diff_without_raw_tool_input() {
    let params = json!({
        "toolCall": {
            "toolCallId": "private-id", "title": "Write note.txt", "kind": "edit",
            "rawInput": {"token": "private-token"},
            "locations": [{"path": "/project/note.txt"}],
            "content": [{"type": "diff", "path": "/project/note.txt",
                "oldText": "before\n", "newText": "after\n"}]
        },
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
        "_meta": {"permission": {"version": 1, "title": "Write note.txt",
            "description": "Review this edit"}}
    });
    let (public, event) = permission(&params, None, &PaneNames::new()).expect("valid permission");
    assert_eq!(event, "pane.permission_requested");
    assert_eq!(public["title"], "Write note.txt");
    assert_eq!(public["detail"], "Review this edit");
    assert_eq!(public["presentation"]["targetPath"], "/project/note.txt");
    assert_eq!(
        public["presentation"]["fileChanges"][0]["oldText"],
        "before\n"
    );
    assert_eq!(
        public["presentation"]["fileChanges"][0]["newText"],
        "after\n"
    );
    let encoded = public.to_string();
    assert!(!encoded.contains("private-token"));
    assert!(!encoded.contains("private-id"));
    assert!(!encoded.contains("rawInput"));
}

#[test]
fn large_diff_is_not_presented_as_a_complete_preview() {
    let tool = json!({"content": [{"type": "diff", "path": "/project/large.txt",
        "newText": "x".repeat(limits::PERMISSION_PREVIEW_BYTES + 1)}]});
    let presentation =
        permission_presentation(&tool, None, &PaneNames::new()).expect("bounded presentation");
    assert!(presentation.preview_omitted);
    assert!(presentation.file_changes.is_empty());
}

#[test]
fn claude_execute_permission_shows_raw_command_not_summary() {
    // Sanitized from the real claude-acp permission request observed on VM1.
    let command = "touch made-by-shell.txt && ls -la";
    let params = json!({
        "toolCall": {"kind": "execute", "title": "Create file and list directory contents",
            "rawInput": {"command": command, "description": "Create file and list directory contents",
                "secret": "never-public"},
            "content": [{"type": "content", "content": {"type": "text",
                "text": "Create file and list directory contents"}}]},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
        "_meta": {"permission": {"version": 1,
            "title": "Create file and list directory contents",
            "description": "Review shell command"}}
    });
    let (public, _) =
        permission(&params, None, &PaneNames::new()).expect("complete command must be displayable");
    assert_eq!(public["title"], "Create file and list directory contents");
    assert_eq!(public["presentation"]["command"], command);
    assert_eq!(public["presentation"]["executionDetails"], Value::Null);
    assert!(!public.to_string().contains("never-public"));
    assert!(!public.to_string().contains("rawInput"));
}

#[test]
fn execute_permission_preserves_long_multiline_command_and_extra_content() {
    let command = format!("ls -la\nprintf '%s' {}", "x".repeat(600));
    let params = json!({
        "toolCall": {"kind": "execute", "title": "List and print",
            "rawInput": {"command": command},
            "content": [{"type": "content", "content": {"type": "text",
                "text": "Working directory: /project"}}]},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
    });
    let (public, _) =
        permission(&params, None, &PaneNames::new()).expect("complete command must be displayable");
    assert_eq!(public["presentation"]["command"], command);
    assert_eq!(
        public["presentation"]["executionDetails"][0],
        "Working directory: /project"
    );
}

#[test]
fn codex_execute_permission_uses_adapter_command_not_generic_title() {
    // codex-acp 1.11.0 constructs command approvals with a string
    // rawInput.command, even when its internal launcher uses argv.
    let params = json!({
        "toolCall": {"kind": "execute", "title": "List files",
            "rawInput": {"command": "ls -la", "cwd": "/project"}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
        "_meta": {"permission": {"version": 1, "title": "Run command?"}}
    });
    let (public, _) =
        permission(&params, None, &PaneNames::new()).expect("Codex command must be displayable");
    assert_eq!(public["title"], "Run command?");
    assert_eq!(public["presentation"]["command"], "ls -la");
    assert_eq!(public["presentation"]["toolArguments"], Value::Null);
    assert!(!public.to_string().contains("/project"));
}

fn workspace_panes() -> PaneNames<'static> {
    PaneNames::from([("pane-claude", "Claude"), ("pane-codex", "Codex")])
}

#[test]
fn codex_mcp_tool_approval_shows_server_tool_and_arguments() {
    // Shape of the codex-acp request recorded in a real Claude and Codex
    // run: an MCP call arrives as kind "execute" with no command.
    let params = json!({
        "toolCall": {"toolCallId": "exec-d712", "title": "mcp.soyeht-workspace.message_agent",
            "kind": "execute", "status": "pending",
            "rawInput": {"arguments": {"targetPaneId": "pane-claude", "text": "HTTP/2 200 \r\n"},
                "server": "soyeht-workspace", "tool": "message_agent"}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
    });
    let (public, _) = permission(&params, None, &workspace_panes())
        .expect("MCP tool approval must be displayable");
    assert_eq!(public["title"], "Use soyeht-workspace · message_agent");
    assert_eq!(public["presentation"]["command"], Value::Null);
    assert_eq!(
        public["presentation"]["toolArguments"],
        json!([{"name": "targetPaneId", "value": "Claude"},
            {"name": "text", "value": "HTTP/2 200 \r\n"}])
    );
    assert!(!public.to_string().contains("pane-claude"));
    assert!(!public.to_string().contains("rawInput"));
}

#[test]
fn claude_and_codex_mcp_approvals_render_identically() {
    let arguments = json!({"targetPaneId": "pane-codex", "text": "Please rerun it.",
        "attempt": 2, "urgent": false, "tags": ["proof"]});
    let codex = json!({
        "toolCall": {"toolCallId": "exec-1", "title": "mcp.soyeht-workspace.message_agent",
            "kind": "execute", "status": "pending",
            "rawInput": {"arguments": arguments, "server": "soyeht-workspace",
                "tool": "message_agent"}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
    });
    // claude-acp names MCP tools and repeats the name as the title.
    let claude = json!({
        "toolCall": {"toolCallId": "toolu_1", "name": "mcp__soyeht-workspace__message_agent",
            "title": "mcp__soyeht-workspace__message_agent", "kind": "other",
            "status": "pending", "rawInput": arguments, "content": []},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}],
        "_meta": {"permission": {"version": 1,
            "title": "mcp__soyeht-workspace__message_agent"}}
    });
    let present = |params: &Value| {
        permission(params, None, &workspace_panes())
            .expect("MCP tool approval must be displayable")
            .0
    };
    let (codex, claude) = (present(&codex), present(&claude));
    assert_eq!(claude["title"], "Use soyeht-workspace · message_agent");
    assert_eq!(codex["title"], claude["title"]);
    assert_eq!(codex["presentation"], claude["presentation"]);
    assert_eq!(
        claude["presentation"]["toolArguments"],
        json!([{"name": "attempt", "value": "2"},
            {"name": "tags", "value": "[\n  \"proof\"\n]"},
            {"name": "targetPaneId", "value": "Codex"},
            {"name": "text", "value": "Please rerun it."},
            {"name": "urgent", "value": "false"}])
    );
}

/// The codex-acp 1.11.0 approval for an MCP call carries only the id,
/// kind and status; the call itself arrived in the earlier tool_call.
fn bare_codex_mcp_request(id: &str) -> Value {
    json!({
        "toolCall": {"toolCallId": id, "kind": "execute", "status": "pending"},
        "_meta": {"is_mcp_tool_approval": true},
        "options": [{"optionId": "allow_once", "kind": "allow_once", "name": "Allow"}]
    })
}

#[test]
fn codex_mcp_approval_is_presented_from_the_tracked_tool_call() {
    let tracked = json!({"toolCallId": "exec-a8b5", "title": "mcp.soyeht-workspace.message_agent",
        "kind": "execute", "status": "in_progress", "locations": [],
        "rawInput": {"arguments": {"targetPaneId": "pane-claude", "text": "  indented\n  reply"},
            "server": "soyeht-workspace", "tool": "message_agent"}});
    let (public, _) = permission(
        &bare_codex_mcp_request("exec-a8b5"),
        Some(&tracked),
        &workspace_panes(),
    )
    .expect("tracked MCP tool approval must be displayable");
    assert_eq!(public["title"], "Use soyeht-workspace · message_agent");
    assert_eq!(public["presentation"]["command"], Value::Null);
    assert_eq!(
        public["presentation"]["toolArguments"],
        json!([{"name": "targetPaneId", "value": "Claude"},
            {"name": "text", "value": "  indented\n  reply"}])
    );
}

#[test]
fn request_fields_override_the_tracked_tool_call() {
    // The tracked call was reported as MCP, but the request itself names
    // a shell command: the request has the last word.
    let tracked = json!({"toolCallId": "exec-1", "title": "Tool", "kind": "execute",
        "status": "in_progress", "locations": [],
        "rawInput": {"arguments": {}, "server": "soyeht-workspace", "tool": "list_agents"}});
    let params = json!({
        "toolCall": {"toolCallId": "exec-1", "kind": "execute", "status": "pending",
            "title": "Run command", "rawInput": {"command": "ls", "cwd": "/project"}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]
    });
    let (public, _) = permission(&params, Some(&tracked), &workspace_panes())
        .expect("command must be displayable");
    assert_eq!(public["title"], "Run command");
    assert_eq!(public["presentation"]["command"], "ls");
    assert_eq!(public["presentation"]["toolArguments"], Value::Null);
}

#[test]
fn untracked_bare_codex_approval_fails_closed() {
    let error = permission(
        &bare_codex_mcp_request("exec-unknown"),
        None,
        &workspace_panes(),
    )
    .expect_err("an approval with nothing to show must not be presented");
    assert!(error.to_string().contains("no command to display"));
    // A tracked call that is neither a command nor an MCP call stays closed too.
    let tracked = json!({"toolCallId": "exec-unknown", "title": "Tool", "kind": "execute",
        "status": "in_progress", "locations": [], "rawInput": {"server": "soyeht-workspace"}});
    assert!(
        permission(
            &bare_codex_mcp_request("exec-unknown"),
            Some(&tracked),
            &workspace_panes(),
        )
        .is_err()
    );
}

#[test]
fn mcp_arguments_over_display_limit_fail_closed() {
    let params = json!({"toolCall": {"kind": "execute", "title": "mcp.server.tool",
        "rawInput": {"server": "server", "tool": "tool",
            "arguments": {"text": "x".repeat(limits::PERMISSION_COMMAND_BYTES)}}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());
}

#[test]
fn opencode_execute_permission_with_command_uses_command_not_bash_title() {
    let params = json!({"toolCall": {"kind": "execute", "title": "bash",
        "rawInput": {"command": "echo test", "description": "Print a test line"}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    let (public, _) =
        permission(&params, None, &PaneNames::new()).expect("OpenCode command must be displayable");
    assert_eq!(public["presentation"]["command"], "echo test");
    assert!(!public.to_string().contains("Print a test line"));
}

#[test]
fn execute_without_displayable_command_fails_closed() {
    let params = json!({"toolCall": {"kind": "execute", "title": "List directory",
        "content": [{"type": "content", "content": {"type": "text",
            "text": "List directory"}}]},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());

    let params = json!({"toolCall": {"kind": "execute", "title": "List directory",
        "rawInput": {"command": "  \n  "}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());

    // OpenCode has emitted a pending bash permission with rawInput: {};
    // a title of "bash" cannot authorize an undisclosed command.
    let params = json!({"toolCall": {"kind": "execute", "title": "bash",
        "rawInput": {}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());

    // A partial or malformed MCP shape is not a recognised tool request.
    for raw_input in [
        json!({"server": "soyeht-workspace"}),
        json!({"server": "soyeht-workspace", "tool": "message_agent", "arguments": "text"}),
    ] {
        let params = json!({"toolCall": {"kind": "execute", "title": "Run tool",
            "rawInput": raw_input},
            "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
        assert!(permission(&params, None, &PaneNames::new()).is_err());
    }

    // An argv array is not a shell command string; never join it by guesswork.
    let params = json!({"toolCall": {"kind": "execute", "title": "Run command",
        "rawInput": {"command": ["ls", "-la"]}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());
}

#[test]
fn execute_command_over_display_limit_fails_closed() {
    let params = json!({"toolCall": {"kind": "execute", "title": "Run command",
        "rawInput": {"command": "x".repeat(limits::PERMISSION_COMMAND_BYTES + 1)}},
        "options": [{"optionId": "yes", "kind": "allow_once", "name": "Yes"}]});
    assert!(permission(&params, None, &PaneNames::new()).is_err());
}
