# ACP v1 client

This crate handles native JSON-RPC correlation, record offsets, session
configuration and v1 response/update interpretation. The engine owns durable
state and effects; the process supervisor owns pipes and process lifetime.

```sh
cargo test --manifest-path engine/Cargo.toml -p soyeht-acp
```

The tests launch `soyeht-fake-acp` with a disposable `--state-dir`.
Its normal prompt echoes text in chunks. The deterministic test prompts are:

| Prompt | Behavior |
|---|---|
| `slow` | thirty delayed chunks, cancellable |
| `permission` | request command permission |
| `permission-edit` | request edit permission |
| `permission-null` | permission request with a valid null JSON-RPC ID |
| `elicitation` | request a form, even in allow-all mode |
| `tools` | request a file write/read and a small client terminal |
| `die` | exit this test process with code 17 |

`--load supported|unsupported|advertised-only|missing|timeout` selects resume
behavior. `--require-login` requires a separate invocation with `--login`.
`--version-label` changes the reported version for fingerprint tests. Nothing
connects to a model service, and the peer is not a production harness.
