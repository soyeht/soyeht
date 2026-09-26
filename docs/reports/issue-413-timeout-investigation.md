# Engine timeout after an agent prompt (#413)

## Evidence checked

- The original sign-in proof reported a client timeout modal after the agent had answered. Its engine operation record shows `pane.prompt` replied in 1 ms. Engine processing of that prompt did not consume the client's 12-second deadline; response delivery to the client remains unverified.
- The same proof recorded later `model.list` and `harness.list` replies around 4.4 seconds and a connection `write_error`. Those records did not name the timed-out client request.
- The existing client timeout diagnostic (method and elapsed time, without request payload) was queried from VM1's unified log. Two records at the same second on 2026-09-25 identify `pane.draft` at 12,263 ms and `harness.list` at 12,431 ms. This identifies the RPCs that timed out in that later observation; it does not establish that both were part of the original proof.
- The VM1 engine log contains 77 `pane.draft` operations with a maximum recorded processing duration of 8 ms and 78 `harness.list` operations with a maximum of 8,952 ms. The operation log has no timestamp or shared request ID that links one of these entries to either client timeout. The shorter engine durations suggest time outside engine processing, but do not distinguish queueing, transport, response delivery, or another client delay.
- A separate headless engine test run recorded 59 `pane.prompt` calls (maximum 34 ms), 12 `harness.list` calls (maximum 1,284 ms), and 6 `model.list` calls (maximum 151 ms). It did not reproduce a 12-second operation or the modal. This does not exercise the macOS window or authenticated session.

## Remaining identification step

The client timer in `EngineClient.requestResponse` already records the timed-out method, elapsed time, and a validated pane ID in a private diagnostic category. The engine's `engine.operation` log records an independent operation ID, method, outcome, and processing duration while excluding wire request IDs and parameters. The two streams therefore cannot identify where the extra time accrued for either specific request. A GUI reproduction should correlate a client request with the matching engine operation and connection-close record using a bounded, non-sensitive correlation value. Do not record request parameters, prompt text, credentials, or account data.

No source fix is claimed. The timed-out RPC names are known for the later VM1 observation, but the cause of their delay is not. GUI correlation remains open for reviewer QA on VM1.
