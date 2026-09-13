# ADR 0057: Page observation boundary

- Status: accepted
- Date: 2026-09-13

## Decision

The Mac web host observes console calls and page-initiated `fetch` and
`XMLHttpRequest` completion in the page content world. It sends only the
metadata named by the protocol to a randomly named host handler. The host
validates every field again before adding the entry to an in-memory bounded
ring. It never receives or persists request bodies, response bodies, or
headers.

`web_wait {networkIdle:true}` means that the document is complete, no
instrumented page request is active, and the page has been quiet for 500 ms.
The host also supports bounded text and selector conditions in the same poll.
The public timeout remains capped at 60 seconds.

Initial document resources are covered by `document.readyState`; requests
made through page APIs are covered by the active-request counter. WebKit owns
the actual network stack. This slice does not add per-resource interception.

## Consequences

Console and network history disappears with the host session, as required.
Pages cannot make the host return an unbounded entry because handler input,
each field, ring capacity, cursor page, and final encoded response all have
separate limits. A page can emit false console or request metadata inside its
own pane; callers must treat this output as page-provided diagnostics rather
than trusted engine facts.
