import Foundation
import SoyehtProtocol

@testable import SoyehtClient

/// The conversation of the approved Codex frame ("Interior da pane · APROVADO"): a peer
/// message, a reply, two tool rows, a pending permission and the "High · …" composer chip.
@MainActor enum PaneInteriorFixture {
  static let paneId = "pane-codex"

  static func client() -> EngineClient {
    let client = EngineClient(socket: "/tmp/sn-pane-interior/engine.sock")
    client.upsertPane(
      Pane(
        paneId: paneId, workspaceId: "workspace", name: "Codex", cwd: "/project",
        mode: .agent, state: .waiting_for_you, source: .protocol, confidence: 1,
        phrase: "Needs you", generation: 1, cols: 80, rows: 24, record: true,
        agent: session))
    var view = AgentPaneView()
    view.messages = [
      message(
        "1", role: .user,
        text: "Please run cargo test -p acp contract_session_new and tell me if it passes.",
        agentMessage: AgentMessageSource(paneId: "pane-claude", name: "Claude", harness: "claude")),
      message("2", role: .assistant, text: "Running the contract tests now."),
      tool("3", kind: "read", title: "schema/v2/meta.json", status: .completed),
      tool(
        "4", kind: "execute", title: "cargo test -p acp contract_session_new", status: .in_progress),
      TranscriptMessage(
        localId: "5", generation: 1, seq: "5", role: .assistant, kind: .usage, text: "",
        complete: true, usage: TokenUsage(totalTokens: "61000", contextSize: "200000")),
    ]
    view.permission = PermissionRequest(
      requestId: "request-1", paneId: paneId, generation: 1, promptId: "prompt-1",
      title: "Codex asks to run", detail: "",
      presentation: PermissionPresentation(
        command: "$ cargo test -p acp contract_session_new", fileChanges: [],
        previewOmitted: false),
      options: [
        PermissionOption(optionId: "allow", kind: .allow_once, name: "Allow"),
        PermissionOption(optionId: "always", kind: .allow_always, name: "Always in this pane"),
        PermissionOption(optionId: "deny", kind: .reject_once, name: "Deny"),
      ],
      expiresAt: "2026-10-02T00:30:00Z")
    client.agentPanes[paneId] = view
    return client
  }

  private static let session = AgentSession(
    harness: "codex", version: "1", fingerprint: "fingerprint",
    options: [
      ConfigOption(
        id: "thought_level", name: "Effort", category: "thought_level", type: .select,
        currentValue: .v0("high"),
        options: [
          ConfigChoice(value: "medium", name: "Medium"), ConfigChoice(value: "high", name: "High"),
        ]),
      ConfigOption(
        id: "mode", name: "Mode", category: "mode", type: .select,
        currentValue: .v0("ask"),
        options: [
          ConfigChoice(value: "ask", name: "Ask for approval"),
          ConfigChoice(value: "edits", name: "Accept Edits"),
        ]),
    ],
    authMethods: [], queue: [], draft: false, role: .worker)

  private static func message(
    _ id: String, role: TranscriptRole, text: String, agentMessage: AgentMessageSource? = nil
  ) -> TranscriptMessage {
    TranscriptMessage(
      localId: id, generation: 1, seq: id, role: role, kind: .message, text: text,
      complete: true, agentMessage: agentMessage)
  }

  private static func tool(
    _ id: String, kind: String, title: String, status: ToolStatus
  ) -> TranscriptMessage {
    TranscriptMessage(
      localId: id, generation: 1, seq: id, role: .assistant, kind: .tool, text: "",
      complete: status == .completed,
      tool: ToolCall(toolCallId: id, title: title, kind: kind, status: status, locations: []))
  }
}
