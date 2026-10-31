# ADR 0130: The WebSocket listener refuses web pages

- Status: accepted
- Date: 2026-10-31

## Context

The engine always listens on its profile's port on the loopback address, and on the tailnet
addresses when there are any. The listener accepted any WebSocket handshake. A web page open in a
browser on the same machine can open `ws://127.0.0.1:<port>` (a browser applies no same-origin rule
to a WebSocket), so any page the person visited could reach everything the engine does before a
client signs in: the handshake, the pairing messages, and the connection slots (ADR 0129). A page
could not sign in, but it should not be able to try, and every bug in the code before sign-in
would have been reachable from the open web.

## Decision

A handshake that carries an `Origin` header is answered `403 Forbidden`. A browser sends the
page's origin on every WebSocket it opens and a page cannot leave it out or change it; none of the
engine's clients is a page, and none sends one: the iPhone and iPad use `URLSessionWebSocketTask`,
which sends no `Origin` (checked on the request it writes), and the test clients are native.

This does not change who can sign in (a paired device, with its key), and it does not stop a
program on the machine, which can write any header. It closes the one route by which a web page,
which is not a program the person started, reaches the listener.
