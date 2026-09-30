# Contract 06 · iPhone and iPad

Status: SETTLED on 2026-09-10. Caio's decision: “follow what exists today;
it has been confirmed to work”. Evidence of today's behavior is in
(internal analysis, outside the repository) (“How the iPhone reaches the Mac TODAY”).

## 1. Rules (same as today, carried into the new engine and app)
1. **Network**: the engine listens on the Soyeht port at Tailscale addresses
   (v4 and v6) and loopback. On the LAN only while the “Add iPhone” window is
   open. Never `0.0.0.0`. Bonjour advertisement on the LAN and tailnet.
2. **Transport**: plaintext HTTP and WebSocket within the tailnet;
   encryption is provided by Tailscale. The new iOS app carries the same ATS
   exception (`NSAllowsArbitraryLoads`) with the same explanatory comment.
   Optional evolution without changing the rest: HTTPS/WSS through `tailscale serve`.
3. **Pairing**: QR (or URI) with address + single-use code; the app opens the
   connection and answers an HMAC challenge; the Mac requests owner consent;
   refusals include a reason (expired, already used, invalid challenge, denied,
   revoked). Once per device; revocable in preferences.
4. **Protocol**: the SAME event and request contract as other clients
   (contracts 03 §12 and 04 §9); the paired device is a view of the Mac session.
5. **Reconnection**: on returning from the background, the app requests full
   state (panes, tasks, pending permission requests) before displaying anything;
   never just reopen the socket (lesson from internal analysis).
6. **Push**: the engine on the Mac sends directly to APNs (as today) for:
   permission requests, tasks marked “your turn”, “no signal”, pane death.
   Live Activity for “agent running” (already exists for deployment; reuse it).
7. **What the iPhone and iPad do in v1**: everything the person can do at the Mac.
   A paired iPhone or iPad is a full remote control (owner decision,
   2026-09-26): view workspaces, panes and state; type into raw terminals
   (software keyboard with Esc, Ctrl, Tab and arrow keys, or a hardware
   keyboard); drive agents, answer permission requests, send text; task board
   (view, drag, dispatch); yolo button; change model/effort; create, close and
   move panes and workspaces; change settings. The engine grants the paired
   device the same role powers as the local human (contract 08 §2.4); a phone
   screen that lacks a control for one of these is a missing feature, not a
   permission boundary.
8. **Devices**: one iOS app serves iPhone and iPad, pairs the same way and has
   the same powers. At regular width (iPad full screen or a wide Split View or
   Stage Manager window) it shows workspaces and panes in a sidebar next to the
   selected pane; at compact width it keeps the list that pushes a pane.

## 2. Outside v1
Cloud relay; connections without Tailscale; a custom mesh (10.44/16) for
paired devices.

## Review applied (2026-09-10, finding R10)
The QR → challenge → consent → `deviceToken` path is defined in contract 08 §2.2; push and Live Activity in 08 §2.3; revocation invalidates the token, connections and push.

## Owner decision (2026-09-26)
The earlier rule that the iPhone reads terminals without typing was never
agreed by the owner and is superseded: a paired device has the same powers as
the local human, including live typing into the raw terminal (role matrix,
08 §2.4). Pairing consent on the Mac and the signed `hello` remain the only
gate.
