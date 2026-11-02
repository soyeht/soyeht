# ADR 0129: The ways in from outside the Mac are narrowed

- Status: accepted
- Date: 2026-10-31

## Context

A paired device is a full remote control of the Mac (ADR 0013), so everything that stands between
the outside and a signed-in device is part of the product's security: the engine's TCP listeners
(loopback and the tailnet), the app that pairs, and the connection slots before anyone has signed
in. A review before launch found four places where that edge was wider than it needed to be. Pairing
itself is decided separately (ADR 0128). Each rule below is one decision with its own change; they
are recorded together because they are one boundary.

## Decision

**1. The local endpoint and the TCP listeners keep separate connection slots.** They shared one pool
of 128. A connection that has said nothing keeps its slot for the handshake deadlines (ten seconds),
so enough idle connections to a listener left none for the Mac app, the command line or an agent.
`Admission` (`soyeht-engine/src/admission.rs`) gives the local endpoint a pool of its own (128) and
the listeners a second one (128), and one source address holds at most 16 places of it (a paired
device keeps at most 8 connections, and each may be mid-handshake while an older one closes). An
address is compared in its canonical form, so an IPv4 address and its IPv6-mapped spelling are one;
an IPv6 address is counted as the one address it is (a /128), not as the /64 it belongs to. The cap
is a fairness rule among peers that each have one address, not a defence against a flood from many
addresses (a tailnet gives a node one address, but a peer on a larger prefix could spread over it):
that is the work of the pool, which no flood from the listeners can empty for the local endpoint.
A place is freed when the connection ends. The limits are constants, not settings.

**2. The WebSocket listener refuses web pages.** A handshake that carries an `Origin` header is
answered `403 Forbidden`. A browser puts the page's origin on every WebSocket it opens and a page
cannot leave it out; none of the engine's clients is a page, and none sends one (the iPhone and iPad
use `URLSessionWebSocketTask`, which sends no `Origin`, checked on the request it writes). A program
on the machine can still write any header; the rule closes the route by which a web page, which is
not a program the person started, reaches the listener.

**3. The engine's TCP listeners are exclusive on Windows.** There, a program of any user can bind an
address that another listens on if it asks for `SO_REUSEADDR` and the first did not ask for
`SO_EXCLUSIVEADDRUSE`. `soyeht_platform::net::listen` makes every TCP listener of the engine: on
Windows a socket with `SO_EXCLUSIVEADDRUSE` set before the bind (IPv4 and IPv6, the code the local
endpoint already used), elsewhere the standard listener. The local endpoint's loopback socket uses it
too, so there is one place for the rule. The engine turns the listener into a `tokio` one
(`network::listen`) at its two bind sites.

**4. A tapped pairing link asks before it pairs.** The iPhone and iPad app registers the
`soyeht-next` scheme, and it paired as soon as a link opened it. Pairing replaces the Mac the device
controls. A link that reaches the app by a tap is held as an `IncomingPairing` (`apps/shared`) and
the app asks "Pair with the Mac at `<host:port>`?", saying when the device would stop controlling the
Mac it is paired with. A link that is not a pairing link, or names an address the parser refuses, is
refused before anything is asked. The camera and the paste field are the person's own act and keep
pairing at once. The Mac still has the last word (ADR 0128).
