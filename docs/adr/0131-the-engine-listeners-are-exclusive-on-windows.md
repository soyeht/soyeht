# ADR 0131: The engine's TCP listeners are exclusive on Windows

- Status: accepted
- Date: 2026-10-31

## Context

On Windows a program of any user can bind an address and port that another program already
listens on, if it asks for `SO_REUSEADDR` and the first program did not ask for
`SO_EXCLUSIVEADDRUSE`. The new socket is then handed the connections meant for the first. The
standard library and `tokio` ask for neither. The local endpoint already avoided this for its
loopback socket (ADR 0086); the engine's TCP listeners, on loopback and on the tailnet addresses,
did not, so on a machine with more than one user another user could take the port the phone dials.

## Decision

`soyeht_platform::net::listen` makes every TCP listener of the engine: on Windows a socket with
`SO_EXCLUSIVEADDRUSE` set before the bind (IPv4 and IPv6, the code the local endpoint already used),
and elsewhere the standard listener, where a second bind of a used address fails. The local
endpoint's loopback socket uses it too, so there is one place for the rule. The engine turns the
listener into a `tokio` one (`network::listen`) at its two sites, the fixed `--listen` addresses and
the tailnet addresses it follows.
