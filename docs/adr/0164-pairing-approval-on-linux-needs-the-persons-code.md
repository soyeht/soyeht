# ADR 0164: Pairing approval on Linux needs the person's code

- Status: accepted (the owner chose this option, "(b)", for the open question of ADR 0162 on 2026-11-14, revised after an independent security review the same day, and chose to launch it without the account password)
- Date: 2026-11-14
- Amends ADR 0128, 0162 and 0163 for Linux. Builds on ADR 0152 (the comparison code). Follows ADR 0161 (the protocol only grows) and ADR 0079 (nothing blocks under the Core lock).

## Context

ADR 0162 made `device.pair.decide` and the revocation of another device the signed app's alone, and
ADR 0163 did the same for `device.pair.start`. On macOS the engine attests the app by its code
signature (`ui_attestation`). Linux has no operating-system code signing, so there nothing counts as
the app and the three methods are refused: no phone can be paired with a Linux computer at all.
ADR 0162 left two ways open: check the path of the installed executable, or ask the person for an
action that no process of the same user can fake. The path check only raises a cost: a same-user
process can start that binary under a debugger or a preload, or replace it when it was installed
for one user. The owner chose the second way.

### The threat

| Attacker | Can |
|---|---|
| **A program of the same user on the computer**, most of all one in a terminal pane that an already-paired (possibly stolen) device opened, or an agent that was talked into running it | connect to `engine.sock` as the local human; read every file of the user, the engine's database included; start processes; use the user's ssh keys and every network path the user has; on X11 read the screen and inject keys |
| **A malicious new device**: a peer of the tailnet, or anything that learned a pairing code | reach the pairing WebSocket, ask to pair, choose its own name |
| **An attacker on the tailnet** | reach the engine's tailnet listener; nothing without a device credential or a live pairing code |

What has to hold: such a program must not be able to approve a new device or revoke one *through the
engine*, and a new device must not be approvable by a tap the person did not mean. A pairing gives
the device every power of the person at the computer (ADR 0013), so a device paired behind the
person's back is a door that outlives the program that opened it.

### What a code can and cannot do

Whatever the engine tells "the app" over the local socket, a same-user program can ask for too: the
socket is the same for both and the engine cannot tell them apart. So **no secret can stop a
same-user program that is also the new device**: it would know the code of its own request and
approve it. The code of this ADR protects the person's tap against a requester that is not in their
hands (a malicious new device, a tailnet attacker, a phishing prompt). What stops a program *on this
computer* from being the requester is the set of rules about where the new device connects from (the
"new device is another node" section). The two parts need each other, and this ADR is explicit about
which part does which job.

## Decision

### The modes

The engine has one rule, `pairing::Approval::for_this_system()`, **chosen by platform only**, never
by a failed attestation:

- **`SignedApp`** on macOS: nothing changes. The three methods are the signed app's alone (ADR 0162,
  0163), there is no code, no node rule and no new store write, and the phone shows the comparison
  code of ADR 0152 as before. An unsigned development engine on macOS stays in this mode and keeps
  refusing, with the refusal and the log line it has today (`engine_cannot_attest`).
- **`PersonsCode`** on Linux: the rules below.
- **`Refused`** on Windows, as ADR 0162 has it, until its app proves itself with Authenticode. The
  same positive rules would work there without `SO_BINDTODEVICE`, but they would be weaker, the
  certificate is on its way, and a second Windows rule set to retire later is not worth it. This ADR
  recommends declaring Windows pairing out of scope until Authenticode. The desktop pairing view is
  shared and follows the capability, so it shows "cannot pair yet" on Windows as it will on any
  engine without `pairing-code`.

A `Hooks` method (`pairing_approval`, default "the platform's") lets a test choose the mode, so the
macOS gate runs the Linux rules and the Linux run (`scripts/check_engine_linux.sh`) runs them for
real. The engine says which mode it is in with a new `EngineCapability`, `pairing-code`, in the
`capabilities` of `welcome`; a client that does not know the name ignores it. The desktop sends
`approvalCode` only when that capability is present. A `device.pair.decide` with `approvalCode` in
`SignedApp` mode is still the signed app's alone, and the field is ignored.

### The flow (variant A: the phone shows the code, the person types it at the computer)

1. The person chooses "Pair a phone" in the desktop app. The app calls `device.pair.start` and shows
   the QR with the window code, as the Mac does (ADR 0153). The window code goes back only to the
   caller of `device.pair.start`; no event, snapshot or listing carries it.
2. The phone scans, then runs `pair.begin` and `pair.prove` as today (contract 08 §2.2), from another
   node of the tailnet (next section).
3. When `pair.prove` verifies, the engine makes an **approval code**: six decimal digits from the
   system's random source, uniform (rejection sampling). It lives **in engine memory only**, in a
   newtype with no `Debug`, no `Serialize` and no `Clone`, zeroized when dropped, and it is **never a
   field of `Pending`** (which is serialised into the store), of a receipt, of a log line or of an
   event. It is kept with the request's deadline and an attempt count. The engine sends it to the
   phone, in the answer to `pair.prove`, as a new optional field `approvalCode`, **only on a
   connection that passed the rules of the next section**, and adds it after the stored result is made
   (as `device.pair.start` adds `reachability`), so a replay of the call gets the same code from memory
   and the stored record never holds it. If the engine restarted and the code is gone, a replay that
   verifies makes a new one.
4. The phone shows the code, large (`482 913`), and under it the comparison code of ADR 0152, and
   says to type the code in Soyeht on the computer.
5. The desktop app shows the request with the device's name, the comparison code and a field for the
   code. It never receives the approval code. The person finds the request whose comparison code is
   the same as the phone's and types the six digits.
6. The app calls `device.pair.decide {requestId, approve: true, approvalCode}`. In `PersonsCode` mode
   the engine approves only if all of these hold: the connection is the local human **and is the one
   that opened the request's window** (see the section on windows); the request is pending and its
   window active; the request has a code in memory, not past its deadline, with fewer than three
   wrong attempts; and the submitted digits equal it (constant-time comparison). A missing or wrong
   code answers the new error code `pairing_code_invalid` and approves nothing. Only six ASCII digits
   that do not match count as a wrong attempt: a missing code or a text that is not six digits can
   never be right, so it costs no attempt and a typing slip does not deny the person; the third wrong code
   **denies the request** and the phone gets `pair.result` with `approved: false`. The code is single
   use: dropped when used, when the request is decided and when it expires. The code is checked
   before the call is recorded, so a wrong code leaves no receipt, and **`approvalCode` is left out of
   the parameters whose hash is the receipt's fingerprint**, so the stored record holds nothing
   derived from it (a six-digit code under a plain hash is an offline lookup).
7. `device.pair.decide` with `approve: false` needs no code in either mode (denying is the safe
   direction) and, in `PersonsCode` mode, the local human that opened the window.

What the code is for: it makes a tap not enough. The person types digits that only the pairing phone
shows, so a prompt from a device that is not in their hands cannot be completed, and a tailnet
attacker or a malicious app on another machine cannot get approved by a mistaken Allow. It is also
the person's own action in the sense of the owner's decision. It is **not** what keeps a program on
this computer out: that program could read the code of any request it can create, because it would be
the device. The code travels inside the phone's tailnet connection and is never on the computer's
API, files, screen or log; a program that captures packets on the tailnet interface (a host where
`dumpcap` has network capabilities and the user is in its group) can read it in transit, which lets
it approve the person's own request and nothing more.

### The new device is another node of the tailnet

This is the rule that keeps a program on this computer from being the device. In `PersonsCode` mode
`pair.begin` and `pair.prove` are served only when every one of these holds, checked first thing in
`pair_request`, **before** `idempotency_receipt` (a replay of `pair.begin` from a local program gets
no stored answer) and again in `pair.prove`:

1. **The connection was accepted by a tailnet listener.** `Connected` records which listener accepted
   it (the local endpoint, loopback, a fixed `--listen` address, or a tailnet listener). The others
   get no pairing. On Linux the tailnet listener is bound with `SO_BINDTODEVICE` to the tailnet
   interface, so a local program that connects to the tailnet address arrives through the loopback
   interface and is not accepted at all (to be verified on the Linux VM, including that the follower
   that rebinds on address changes keeps it).
2. **The peer is what `TcpStream::peer_addr()` says**, canonicalised (`to_canonical()`, so
   `::ffff:127.0.0.1` is loopback). Never a header: nothing in `X-Forwarded-For`, `Forwarded` or a
   PROXY line is read. The WebSocket handshake carries no identity.
3. **The peer address is in the Tailscale ranges**, `100.64.0.0/10` or `fd7a:115c:a1e0::/48`, and is
   **not an address of any interface of this machine**, from a fresh full `getifaddrs` at the time of
   the call (not the two-second list of the tailnet follower, which holds only tailnet interfaces; a
   program can bind a LAN, docker or link-local source address). An optional probe tries to bind the
   peer address and refuses when it succeeds. Any doubt (an error listing, an unparseable address)
   refuses.
4. **Tailscale says the peer is another node.** The engine asks the local `tailscaled` (LocalAPI
   `whois` of the peer address) which node it is, and requires a stable node id that is not this
   node's. When `tailscaled` cannot be asked, there is no pairing: it fails closed, and the refusal
   says that Soyeht could not ask Tailscale who is connecting. The call is blocking IO, so it goes
   through `Outcome::Offload` and runs before the request reaches the Core (ADR 0079). Whether an
   unprivileged user may call `whois` on a default install is to be verified on the Linux VM.
5. **The tailnet identity has not changed.** The engine pins the tailnet (`CurrentTailnet`) and this
   node's key when it first serves a pairing, and refuses `pair.begin` once either differs, until the
   person has been shown the change in the desktop app and confirmed it. This **raises a cost only**:
   the confirmation is a local call, so a program that knows it can make it. It makes the lazy attack
   (`tailscale up` into a tailnet of the attacker's, which a user who is the operator may do without a
   password) show up in the app instead of passing silently.
6. **The node is not a device's node, by node identity and not by IP.** The store keeps the set of
   node ids from which each device's authenticated `hello` came (a table, written only in
   `PersonsCode` mode; the same `whois`, which does not block the `hello` when it fails: the node is
   then simply not recorded). `pair.begin` is refused from any node in that set for a paired device,
   so a stolen phone cannot pair a "second device" from itself, by the Soyeht app or any other program
   on it, over IPv4 or IPv6. The node of a **revoked** device stays refused for 24 hours after the
   revocation, with no unblock action (an unblock would be a local call a program can make). Permanent
   would deadlock the person who reinstalls the app and has to revoke the old record before pairing
   again; 24 hours covers the time to react, and the revoke dialog closes the device's panes (below),
   which is what ends the program it left behind. Pairing from the same node after that is allowed.
   Existing records: Linux could not pair at all before this ADR, so there are none. A data directory
   that was paired in another mode is unsupported; a device with no recorded node is bound at its next
   `hello`, and the rule compares only recorded nodes.
7. **Each request is bound to its node.** The node of `pair.begin` is kept on the pending request and
   `pair.prove` must come from the same node, so a request cannot be begun from one place and proved
   from another.

Refusals answer `unauthorized` with a message that says why, and write one engine.pair_refused log
line with a kind only (`listener`, `address`, `local_address`, `whois_unavailable`, `same_node`,
`node_changed`, `device_node`, `revoked_node`, `tailnet_changed`), never an address or a name.

By design, **userspace networking** (a `tailscaled` that has no tunnel interface, which also means
the engine has no tailnet listener), **`tailscale serve`** and **Funnel** (which deliver a connection
from loopback) get no pairing. A person who uses them is told so in the refusal and in the docs.

Tests and the headless emulator run (`docs/apps-android.md`) reach the engine from loopback, which
this refuses. They use a port the engine reads its peer rules through (`PairingPeers`: interfaces,
`whois`, listener), which `soyeht-engine-test` implements with fixtures and the shipped engine
implements with `getifaddrs` and the LocalAPI (ADR 0091). The Android emulator cannot pair with a
real Linux engine; that is a test-only path.

### Pairing windows and the device name

**Live windows are bounded and bound.** `open_pair_window` has no limit today. In `PersonsCode` mode
at most four windows are live per local connection (the fifth closes that connection's oldest), and
each window is bound in memory to the local connection that opened it: only that connection may
decide the requests that come in on it, and the snapshot lists only those to it. The desktop sheet
keeps one connection for the whole pairing; a reconnect restarts the pairing; an engine restart
voids the bindings. This stops a program from deciding, or reading the request list of, a window the
person's app opened, and from replaying a window code read from the database. It does not stop it
from opening windows of its own.

**The device name is sanitised** at `pair.begin` before it is stored or shown: at most
`deviceNameBytes`, no control character and none of the bidirectional controls (U+061C, U+200E,
U+200F, U+202A to U+202E, U+2066 to U+2069), else `invalid_params`.

The earlier rule that the approval code is unique among the pending requests is dropped: a code is
checked against its own request, so uniqueness buys nothing.

### Start, revoke and the table

| Call | `SignedApp` (macOS) | `PersonsCode` (Linux) |
|---|---|---|
| `device.pair.start` | the signed app | the local human |
| `device.pair.decide`, approve | the signed app | the local human that opened the window, with the person's code |
| `device.pair.decide`, deny | the signed app | the local human that opened the window |
| `device.revoke` of another device | the signed app | the local human |
| `device.revoke` of itself | the paired device | the paired device |
| any of the above | a paired device (other than its own revocation), an agent, a chief: `forbidden_role` | the same |

`device.pair.start` is open to the local human here because a window alone no longer puts a prompt in
front of the person that they can complete: a window opened by a program has a link that only
another node can use, and nothing for the person to type. (ADR 0163's concern, a prompt with a name
of the attacker's choosing, now needs a node of the tailnet that the attacker controls.)

`device.revoke` needs no code. There is none to ask for: the device being revoked is the one that
shows it, and a lost phone shows nothing; and revocation is what the person reaches for when a phone
is stolen, so it should not be hard. **Revoking a phone does not stop the processes that its
terminals started.** The revoke dialog therefore lists the panes that device created (from the actor
that the engine's events already record; slice 4 checks the attribution is complete) and offers to
close them. Closing a pane ends its process group, not a process that detached itself; the dialog
says so.

`send_pair_result` says "Denied on Mac" today; in `PersonsCode` mode it says "Denied on the
computer" (the Mac keeps its text).

### Detection

Pairing on Linux cannot be made impossible for a program with another node (below), so it must not
be silent. A device record gets `addedAt` and `node` (the tailnet node's name from `whois`), both
optional and additive, and:

- the desktop device list shows them for every device;
- a phone that connects after a device was added since its last connection shows a durable notice,
  "A device was added: name, node, time", until the person dismisses it (the engine also publishes
  the existing `device.paired` event live);
- the notice is the same on the desktop at the next start.

### Copy

Phone (iPhone and iPad; `PairingSession` reads `approvalCode`; when the answer has none, which is
every pairing with a Mac, it shows what it shows today). The strings go in the `texts` of
`x-clientPolicy`, which the Rust and Swift clients both read; the keys are the contract slice's:

- Title: "Type this code on your computer".
- Under the code: "Open Soyeht on your computer, choose Pair a phone and type the code there. The
  computer shows the same code beside the request: E3B0 C442 98FC 1C14. If it shows another one, or
  none, tap Cancel."
- After the third wrong code: "The code was not accepted. Start again on the computer."

Desktop (Linux now; Windows later, with the same view, following the capability):

- The sheet: "Pair a phone". "Open Soyeht on your iPhone or iPad and scan this code. Both devices
  must be on the same tailnet." The QR, a "Copy link" action, the time left. On Windows, without the
  capability: "This computer cannot pair a phone yet."
- A request: "Allow “<name>”?", the comparison code, "Type the code your phone shows." and a
  six-digit field; Deny beside it. Under it: "Type a code only if your own phone is showing it now,
  for a pairing you started. Nobody else will ask for it."
- A wrong code: "That code is not right. N tries left." The last: "Too many wrong codes. The request
  was denied."
- A request that arrives while the person's pairing sheet is closed is not put in front of them as a
  question; it waits in the device list as "Waiting for approval" with Deny.
- A refusal because of the tailnet (`whois` unavailable, tailnet changed, userspace networking) is
  shown with its reason, with the confirm action for a changed tailnet.
- Devices: the list with `addedAt` and node, and Revoke with the confirmation of the Mac ("Revoke
  this device's access?") plus the list of panes to close.

### The Mac

Nothing changes in what the person sees or what the engine allows: `SignedApp` mode sends no
`approvalCode`, offers no `pairing-code`, writes no node, and `PairApprovalView` is untouched. A new
phone pairing with a Mac sees today's screen because the answer carries no code. The store gets the
node table and the device fields by the same migration on every system (a schema bump on the Mac
too); the Mac never writes them. `ui_attestation::available` stays for the trust of a project folder,
which on Linux still lets any local process count as the app (ADR 0150): the same class of gap, a
separate decision.

## What this stops, and what it does not

Stops, on Linux:

- A same-user program that cannot run code on another node of the owner's tailnet: it cannot be a
  device (the positive rules), so there is no request of its own to approve, and approving the
  person's request needs a code it cannot read. This does not depend on the code or the screen:
  it holds against a program that reads the screen or injects keys on X11.
- A stolen paired phone and the terminal it opened, when the thief has no second node: the phone's
  node is refused by identity on both address families, the computer is refused by the listener and
  the address rules.
- A malicious new device or a tailnet attacker that has the window code: a tap does not approve; only
  the code the pairing phone shows does.
- A phishing prompt from a window that a program opened.

Does not stop (the first is also true of the Mac):

1. **A program that edits the engine's database or the user's files**, the same as ADR 0128: it can add
   a device row with a credential it made.
2. **A same-user program that can run code on any other node of the tailnet**: the user's ssh keys, a
   forwarded agent, Tailscale SSH, a shared directory a node runs. That node pairs, sees the code on
   its screen or in its own process, and tells the program. Nor does it stop a program that runs
   `tailscale up` into a tailnet of the attacker's where the user is the operator (the tailnet pin
   shows it, and a program that knows can confirm it). The Mac stops all of this, because a program is
   never the signed app. This is the real gap between the two systems; the rules above make it need
   another machine and the detection makes it visible, and neither closes it. Only the owner decision
   below closes it.
3. **A program that revokes the owner's other devices** (a denial of remote control, not a gain of
   power; the person at the computer pairs again) or that denies a request or burns its attempts.
4. **Root**, or a debugger in the engine, which reads the code from memory. The engine marks itself
   non-dumpable on Linux (`soyeht_platform`; `PR_SET_DUMPABLE`), which with the usual `ptrace_scope`
   keeps a same-user process from reading it. The flag is cleared by an exec, so the engine sets it
   after it starts, not in a launcher; it does not stop root or a process that already holds the
   engine as a tracee. Nothing relies on it.
5. **A person who types a code an attacker dictates.** The desktop sheet says not to.

Brute force is not a threat, and the reason is not the three tries: a program that can create a
request knows its code, and one that cannot create a request has none but the person's own to guess,
which approving harms no one. Six digits and three attempts are enough for a person typing.

**A same-user program on Linux already has the user's account**: ssh keys, autostart entries, cron,
`~/.bashrc`. Pairing is one more persistence route for it, with the engine's remote tooling, not a
power it lacked. That is context and not an excuse: the gap in item 2 is real, it is the one place
the Mac is stronger, and the owner should decide on it knowing that.

## Owner decision: the account password (polkit)

Only the person's account password, asked by the system's own authentication agent, closes item 2:
the engine would call polkit for an action of its own, the agent in the session asks, the person
types the password, and no process of the user (nor a node of the tailnet) can answer. Costs:

- a policy file that the package installs (the `.deb`, not the AppImage, not a per-user install);
- a polkit agent in the session: none in a headless or minimal session;
- a bus client in the engine (a dependency, ADR 0149) and the password at every pairing, or at the
  first and then for a time;
- on X11 a keylogger of the same user can still read the password.

Decided by the owner on 2026-11-14: the launch ships the person's code with the node rules and the
"device added" notice, without the password. The password may come later as an opt-in ("Require my
account password to pair"); slice 8 is its place, and this ADR is complete without it.

## Compatibility and the protocol

All changes are additive (ADR 0161): the optional `approvalCode` in the answer of `pair.prove` and in
the parameters of `device.pair.decide`; optional `addedAt` and `node` on `Device`; a new
`EngineCapability`, `pairing-code`; a new error code `pairing_code_invalid` (`ErrorKind` is an open
enum); the `texts`; and new cases of an existing refusal (`unauthorized` at `pair.begin`). Nothing is
removed or made required; the Mac's behaviour is unchanged. If this lands before the contract of the
first release is snapshotted (`contracts/released/`) it is part of version 1; after that, `current`
rises to 2 with `min` still 1. The pairing dialogue has no `hello`, so the presence of the field is
the signal: an old phone with a Linux engine never shows the code (the desktop sheet says: update
Soyeht on the phone); a new phone with a Mac sees no field and shows the old screen.

`contracts/vectors/pairing-approval.json` holds the code's shape (six ASCII digits, the grouping
`482 913`, digits accepted with a space) for both clients.

## Rejected and kept in reserve

- **The path of the installed executable** (ADR 0162's first option): raises a cost only.
- **Variant B: the computer shows the code and the person types it on the phone.** The engine would
  send the code to the app, a program can ask for it as the app does, and on X11 it can read it from
  the screen. Variant A keeps the code off the computer.
- **A deny-list of local addresses** (the first version of this ADR): bypassed by a local source
  address that is not on a tailnet interface. Replaced by the positive rules.
- **The ancestry of the peer** (refuse a local connection whose process descends from the
  supervisor): stops a plain child of a pane; a double fork gets out of it. A possible cheap extra.
- **The account password through polkit:** see the owner decision.

## Defaults taken, each easy to change

1. Six digits, three attempts; the code lives until the request's deadline (at most the window's ten
   minutes) and not past an engine restart.
2. Revoke and deny need no code; start needs only the local human.
3. A revoked device's node is refused for 24 hours.
4. Four live windows per local connection.
5. Windows pairing stays refused until Authenticode.
6. Android keeps its core (the shared Rust pairing code carries the new field and an event for the
   code); its screen waits for its own release.

## Slices

| # | Slice | Size |
|---|---|---|
| 1 | Contract: `approvalCode`, `addedAt`, `node`, capability, error code, `texts`, the vector file, generated code, contract 08 §2.2 and §2.4, the status lines of ADR 0128, 0162, 0163, README, `docs/platforms.md`, `docs/apps-android.md` | S, 0.5 day |
| 2 | Engine, mode and code: `pairing::Approval` by platform, the code newtype (zeroized), attempts, receipt without the code, the table above, `pairing_code_invalid`, window cap and binding, name sanitising, "Denied on the computer", the `Hooks` method, non-dumpable | M, 2.5 days |
| 3 | Engine, the peer rules: listener mark on `Connected`, `SO_BINDTODEVICE` on Linux, canonical peer, tailnet ranges, fresh `getifaddrs`, the `PairingPeers` port, a LocalAPI `whois` client off the Core lock, the tailnet pin and its confirmation, the node table and migration, the 24-hour revoked rule, binding a request to its node, the refusal log line. Needs the Linux VM | L, 3.5 days |
| 4 | Detection and revoke: `addedAt` and `node` on the list and notice, the durable "device added" notice on phones and desktop, the revoke dialog's pane list and close | M, 2 days |
| 5 | Desktop pairing view: the sheet, the QR (no encoder today; a permissive-licence crate through the ADR 0149 review), the request row, the field, the device list, the tailnet refusals, `soyeht-client` calls and a `Progress` event for the code in `device::pair` | L, 4 days |
| 6 | Shared Swift and iOS: `PairingSession` reads the code, `PairingCopy` and the view show it, the notice; the Android core passes the field through | S, 1 day |
| 7 | Tests and docs, alongside 2 to 6; `docs/agent-acceptances.md`, `docs/dev-local.md`, `docs/security-testing.md` | M, 2 days |
| 8 | Optional, pending the owner: polkit confirmation | M, 3 days |

Slices 1 to 4 are one engine change and need the second-agent review (pairing, protocol, store,
dependencies, security). The order is 1, 2, 3, 4, then 5 and 6 in parallel.

Tests:

- Engine unit: the table of the three methods by role and mode; wrong code, three wrong codes,
  expiry, single use, restart, the replay of `pair.prove`; the receipt of `decide` holds nothing
  derived from `approvalCode`; the code type has no `Debug` or `Serialize` (a compile-fail test).
- **A local socket client without the person's code is refused**, in `attack_pairing.rs`: the local
  human that is not the app cannot decide with no code, a wrong code or a made-up one; and one that
  did not open the window cannot decide at all. It runs in the macOS gate through the hook and
  natively on Linux (`scripts/check_engine_linux.sh`, `scripts/vmpal_check_engine.sh`), where it
  asserts that the mode is `PersonsCode` with no hook.
- Peer rules: a local source that is not on a tailnet interface is refused; `::ffff:127.0.0.1`, a
  link-local address and a container range are refused; a tailnet address that is one of this
  machine's is refused; the other family of a paired node is refused; a replayed `pair.begin` from a
  local peer is refused; a revoked device's node is refused for 24 hours and allowed after; a
  forwarded header is ignored; `whois` unavailable fails closed; a changed tailnet is refused until
  confirmed; `pair.prove` from another node than `pair.begin` is refused. On the Linux VM, the
  `SO_BINDTODEVICE` listener does not accept a local connection to the tailnet address.
- Windows: the three methods and the pairing stay refused, with the message.
- No leak: a whole pairing, then a scan of every frame a local connection received, the database
  file, the receipts and the log for the approval code.
- Pairing windows and names: a fifth window closes the oldest; a name with a bidirectional control is
  refused.
- Desktop: the pairing state machine against a scripted engine (window, request, wrong code, right
  code, denial, expiry, refusals); sends `approvalCode` only with the capability. Swift:
  `PairingSession` with and without the field, the vector file, the notice. Contract:
  `generate.py --check`, `check_doc_names.py`.
