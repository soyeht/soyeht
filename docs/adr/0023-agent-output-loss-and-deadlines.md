# ADR 0023 · Recover pending requests before acknowledging output loss

Status: accepted for slice 2.

The supervisor's raw output retention and its unanswered-request ledger serve
different purposes. Losing an output page must not erase a permission that is
still waiting for its answer. A stream acknowledgement alone never retires the
supervisor's request ledger.

When a raw gap is detected, first copy affected unanswered requests from the
supervisor's bounded, paginated ledger into the engine's transactional request
store. Preserve an existing public request ID, deadline and answer. A recovered
request uses its original generation and source offset. Do not advance the raw
checkpoint while importing these requests.

Record the gap in the durable raw journal and add a visible system message and
attention item. End any partially assembled text record rather than joining text
across missing output. The first retained byte might be in the middle of a JSON
line. Skip through the next newline, recording this parser state durably across
engine restarts. Recover any pending request overlapping that skipped range
before acknowledging it. Then resume ordinary parsing. An incomplete supervisor
request ledger is a protocol fault; stop the process with an explicit notice.

Output loss never invents a successful prompt response. Preserve an active turn
until its real response arrives, the user cancels it, or its process fails. If
an unobserved response was lost, the visible turn remains unconfirmed and the
user can cancel. Never automatically replay its prompt or effects.

Ninety seconds without protocol traffic raises attention without closing a turn
or answering a question. Human questions keep their own thirty-minute deadline.
Expiration atomically stores the cancellation, native reply, public answered
event and persistent attention item. A new prompt clears the pane's attention.

Cancellation waits ten seconds for the native prompt response. Read already
buffered responses before evaluating that deadline, including after engine
adoption. If no response confirms cancellation, stop the supervised process and
its inline terminals, settle pending calls explicitly, and preserve waiting
prompts. A durable fault state prevents automatic resurrection on restart.

Process exit does not mean its last output page has been consumed. Drain every
remaining page before retiring the process or reporting its terminal output as
complete. This also applies to inline native terminals.

Tests use disposable profiles and the fake native agent. They cover a permission
created during an engine outage, overflow beyond eight MiB with the permission
absent from raw retention, exactly one recovered decision, automatic permissions
never being presented, durable expiration, silence with the same active PID,
and cancellation without confirmation. Deadlines are accelerated by changing
only a stopped test engine's persisted timestamps; production has no clock or
timeout override.
