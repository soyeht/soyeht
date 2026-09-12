# ADR 0043 · Usage reader source eligibility

Date: 2026-09-12
Status: accepted for slice 4

## Context

Contract 10 requires a reader when a provider exposes a subscription window,
but it also requires every reader to be read-only. A command can display a
percentage while still being unsuitable for an unattended engine. It may need
an interactive terminal, create a conversation, write trust state, refresh a
credential, or provide output with no stable machine-readable boundary.

The real-login investigation found both kinds of source. Three providers have
bounded, machine-readable reads that preserve the existing login. Other tools
either expose only local activity totals, expose a window only inside an
interactive session, or could not use the installed login without changing
authentication state.

## Decision

A subscription source is eligible for a production reader only when all of the
following are true:

1. It returns the provider's subscription window, used fraction and reset time.
2. It has a bounded, deterministic response that can be parsed without screen
   scraping or terminal timing.
3. Reading it does not create a conversation, modify trust state, sign in,
   refresh or rotate a credential.
4. A credential, when required, stays inside the source adapter and is sent
   only to the provider's own endpoint.
5. Failure maps to a typed public state without exposing response bodies,
   credentials or account identifiers.

An interactive-only usage screen is evidence that the provider has a window,
but it is not a reader interface. A locally installed client whose existing
login requires reauthentication or an update is also ineligible until a safe
read can be proved. These providers receive no ring. Adding a later reader
requires a new real-login record and tests against a scripted source port.

The first registry contains exactly the three readers named by contract 10 §7.
Their preferred sources passed the real-login investigation, so credential
fallbacks remain implemented only when they can meet the same boundary and are
not exercised merely to prove their existence.

## Consequences

The engine never drives an interactive terminal to collect usage and never
changes a provider login on a refresh timer. A visible percentage is not enough
to qualify a source. This excludes some installed tools from the first registry
while keeping the read-only promise testable and preserving a direct extension
path when a stable source appears.
