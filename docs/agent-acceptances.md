# Agent session acceptances

Build the Rust workspace, then run `python3 scripts/e2e_agents.py --report
.cache/e2e-agents.json`. The local merge gate runs this suite after the six
shell acceptances. The retained workflow has the same step; hosted Actions
remains disabled by project decision.

The suite copies the four built executables into a disposable directory and
loads uniquely named engine and supervisor jobs into launchd. It never uses an
installed profile or an authenticated provider. Its only peer is the deterministic
native test process. Cleanup closes the supervised processes and removes the jobs
and directory, including after a failed assertion.

The ten results cover streamed updates to two protocol clients and paginated
history; first-response permission handling and expiry; healthy cancellation;
a permission created while the engine is absent; confirmed configuration and
native rejection; yolo versus interactive forms; durable queue and draft
priority; supervisor replacement with and without native load; capability cache
invalidation; and sign-in through a separate terminal.

The streaming clients use separate local connections. Swift connection tests
cover device authentication and WebSocket transport; actual Mac/iPhone rendering
and the real-adapter procedure are separate checks. Passing this suite alone
does not establish device UI or real-provider behavior.

An authenticated, opt-in compatibility check uses the pinned official ACP
adapter in a disposable profile:

```sh
SOYEHT_REAL_ACP=1 python3 scripts/e2e_real_acp.py \
  --report .cache/e2e-real-acp.json
```

This command downloads the pinned adapter into a temporary directory. It uses a
temporary project rule to force one harmless shell permission, answers it through
a second engine client, changes and restores the advertised model, exercises yolo,
and kills only the disposable engine while a real turn is active. It removes its
services, files and adapter installation afterward. The command never runs in the
normal merge gate unless `SOYEHT_REAL_ACP=1` is explicitly present. It does not
replace the physical iPhone procedure.

Permission expiry is accelerated by changing only the pending test request's
timestamp while the disposable engine is stopped. No shorter production timeout
or clock override is added. For the engine-outage case, only its uniquely named
job is briefly disabled, killed and removed; the override is re-enabled before
the replacement job is loaded. The supervisor ledger proves that a native
permission exists before the engine returns, and the database proves the engine
had not already received it.

Native load replaces its bounded history after confirmation. The regression
compares the ordered user and assistant messages before and after supervisor
loss, including a cancelled turn with no response text. An empty native replay
chunk must preserve the role boundary without adding an empty visible message.
