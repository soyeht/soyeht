# Engine timeout after an agent prompt (#413)

## Evidence checked

- The sign-in proof reported a client timeout modal after the agent had answered. Its engine operation record shows `pane.prompt` replied in 1 ms. Engine processing of that prompt did not consume the client's 12-second deadline in that occurrence; response delivery to the client remains unverified.
- The same proof recorded later `model.list` and `harness.list` replies around 4.4 seconds and a connection `write_error`. Those records do not identify the request whose client timer fired. A catalogue operation could still have waited in a client queue or competed with other work; the log does not prove that hypothesis.
- I reran the headless engine workspace tests and inspected only allowlisted operation names and durations in their structured logs. The run recorded 59 `pane.prompt` calls (maximum 34 ms), 12 `harness.list` calls (maximum 1,284 ms), and 6 `model.list` calls (maximum 151 ms). None reproduced a 12-second operation or the modal. This does not exercise the macOS window, client timer, or the authenticated Claude session.

## Remaining identification step

The client timer in `EngineClient.requestResponse` currently emits only a generic `ClientFailure.timeout`, removes the pending request, and closes the wire. The engine's `engine.operation` log records an independent operation ID, method, outcome, and duration; it deliberately excludes wire request IDs and parameters. These records cannot be joined to name the timed-out RPC. A GUI reproduction should record the timeout's allowlisted method, a locally generated correlation ID, and elapsed time without request parameters, prompt text, credentials, or account data, then compare it with engine operation and connection-close records. Only then should a fix target the confirmed operation.

No source fix is claimed here. The prompt RPC itself and a consistently slow catalogue operation were not reproduced in the headless run. GUI reproduction remains open for reviewer QA on VM1.
