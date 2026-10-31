# ADR 0129: The local endpoint and the TCP listeners keep separate connection slots

- Status: accepted
- Date: 2026-10-31

## Context

The engine served the local endpoint (the Mac app, the command line, the agents' MCP programs) and
its TCP listeners (loopback and the tailnet) from one pool of 128 connection slots. A connection
takes a slot when it is accepted and keeps it until it closes. One that has said nothing is closed
after the WebSocket handshake deadline and the first-frame deadline, ten seconds in all.

Anything that can reach a TCP listener can open connections without proving anything: a tailnet
peer, any program or user of the same machine on the loopback address, and a page open in the
person's browser. A few hundred idle connections, renewed every few seconds, took every slot, and
`serve_local` then dropped each connection of the Mac app, so the engine could not be used for as
long as the flood lasted.

## Decision

The slots are kept by `Admission` (`soyeht-engine/src/admission.rs`):

- The local endpoint has a pool of its own (128). Nothing that arrives by TCP can take from it.
- The TCP listeners share a second pool (128), and one source address may hold at most 16 of it. A
  paired device keeps at most 8 connections (`hello`), and each may be mid-handshake while an older
  one closes, so 16 leaves a device room; a single address cannot take every slot from the others.
- An address is compared in its canonical form, so an IPv4 address and its IPv6-mapped spelling are
  one address.
- A place is freed when the connection ends, whatever the reason.

The limits are constants of that file, not settings: they bound a hostile caller, and a person has
no reason to tune them.
