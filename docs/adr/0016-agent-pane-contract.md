# ADR 0016 · Agent pane contract

Status: accepted. Scope: slice 2.

Clients continue speaking only the engine protocol. The pane gains an optional
`agent` field with harness, version, fingerprint, session, confirmed options,
pending operation, queue, draft and permission mode. Persisted terminal panes
remain valid without that field.

Each accepted prompt receives a `promptId`; its origin is `human` or `automatic`.
The limit is 64 KiB of text and 64 waiting prompts. `pane.draft` indicates human
text that has not been sent, without transmitting that text. Aggregate state
lives in the snapshot; `pane.queued` also reports the draft.

`pane.update` is a closed union of contract 08's eight types, with message identity,
generation and sequence. Thought, tool, plan, usage and terminal are distinct.
The transcript stores normalized messages, assembles chunks under the same identity
and limits pages to 200 messages or 512 KiB. Larger messages are split into records
of up to 256 KiB.

Permission and elicitation requests have their own identities, generation, turn
and deadline. A late response returns the result that already won; it never
produces another native response. `by` distinguishes Mac, iPhone, rule and system.
No permission mode answers elicitations. Permission modes are the harness's own
(ADR 0070 replaced the engine's `ask`/`accept_edits`/`allow_all` policy and Yolo).
Configuration options accept selection or boolean values.

A v1 session ends with the `session/prompt` response. The implementation follows
the [official v1 schema](https://github.com/agentclientprotocol/agent-client-protocol/blob/2821a8b51dca2aeb3d48b73c9214a6c79968c975/schema/v1/schema.json).
The slice implements v1; an unimplemented version is explicitly refused, without
interpreting v2 messages as v1. V2 adaptation requires its own delivery when the
real harness specified by the objective exists.

Contract 02 provides a package when restarting an unsupported configuration,
but the objective reserves that package for slice 4. In this slice, unsupported
configuration fails with the previous value preserved. Proven process loss attempts
`session/load`; proven session loss opens another session with
`pane.session_lost {recovery: new_session}` and a persisted notice. Context
continuation that has not been implemented is not promised. A parameter error
or timeout is not proof of loss and does not open another session.
