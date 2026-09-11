# Contract 06 · iPhone

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
   (contracts 03 §12 and 04 §9); the iPhone is a view of the Mac session.
5. **Reconnection**: on returning from the background, the app requests full
   state (panes, tasks, pending permission requests) before displaying anything;
   never just reopen the socket (lesson from internal analysis).
6. **Push**: the engine on the Mac sends directly to APNs (as today) for:
   permission requests, tasks marked “your turn”, “no signal”, pane death.
   Live Activity for “agent running” (already exists for deployment; reuse it).
7. **What the iPhone does in v1**: view workspaces, panes and state; read the
   pane (agent and raw terminal); answer permission requests; send text;
   task board (view, drag, dispatch); yolo button; change model/effort.

## 2. Outside v1
Cloud relay; connections without Tailscale; a custom mesh (10.44/16) for the
iPhone; live typing into the raw terminal from the iPhone.

## Review applied (2026-09-10, finding R10)
The QR → challenge → consent → `deviceToken` path is defined in contract 08 §2.2; push and Live Activity in 08 §2.3; revocation invalidates the token, connections and push. Typing into a raw terminal from the iPhone is outside v1 (role matrix, 08 §2.4).
