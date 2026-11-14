# ADR 0164: Pairing approval on Linux needs the person's code

- Status: proposed (design only; the owner chose this option, "(b)", for the open question of ADR 0162 on 2026-11-14, and the design has not been reviewed yet)
- Date: 2026-11-14
- Amends ADR 0128, 0162 and 0163 for the systems where the engine cannot attest the app (Linux, and Windows until its app can prove itself). Builds on ADR 0152 (the comparison code). Follows ADR 0161 (the protocol only grows).

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

Who is on the other side, and what each can do before this ADR (the Linux rows) and after it:

| Attacker | Can |
|---|---|
| **A program of the same user on the computer**, most of all one in a terminal pane that an already-paired (possibly stolen) device opened, or an agent that was talked into running it | connect to `engine.sock` as the local human; read every file of the user, the engine's database included; start processes; on X11 read the screen and inject keys |
| **A malicious new device**: a peer of the tailnet, or anything that learned a pairing code | reach the pairing WebSocket, ask to pair, choose its own name |
| **An attacker on the tailnet** | reach the engine's listeners; nothing without a device credential or a live pairing code |

What has to hold: such a program must not be able to approve a new device or revoke one *through the
engine*, and a new device must not be approvable by a tap of the person that they did not mean. A
pairing gives the device every power of the person at the computer (ADR 0013), so a device paired
behind the person's back is a door that outlives the program that opened it.

### Why a secret that only the engine knows does not work

Whatever the engine tells "the app" over the local socket, a same-user program can ask for too: the
socket is the same for both and the engine cannot tell them apart (that is the whole open question).
And a same-user program can also play the other end: open a window, take the link, pair a device of
its own and approve it, if it can connect as that device. So a code the engine shows the app, or
shows the device, is secret from the program only if the program cannot be that device. The design
below therefore has two parts that need each other: the new device cannot be this computer, and the
secret that approves it goes to the device and to the person's hands, and nowhere on the computer.

## Decision

### The two modes

The engine has one rule, `pairing::Approval::for_this_system()`, which says how a pairing is
approved:

- **`SignedApp`** where `ui_attestation::available()` is true (macOS today): nothing changes. The
  three methods are the signed app's alone (ADR 0162, 0163), there is no code, and the phone shows
  the comparison code of ADR 0152 as before.
- **`PersonsCode`** everywhere else (Linux, and Windows until its app can prove itself, which will be
  its own ADR): the rules below.

A `Hooks` method (`pairing_approval`, default "the system's") lets a test choose the mode, so the
macOS gate runs the Linux rules and the Linux run (`scripts/check_engine_linux.sh`) runs them for
real. The engine says which mode it is in with a new `EngineCapability`, `pairing-code`, in the
`capabilities` of `welcome` (a client that does not know the name ignores it).

### The flow (variant A: the phone shows the code, the person types it at the computer)

1. The person chooses "Pair a phone" in the desktop app. The app calls `device.pair.start` (as
   today) and shows the QR with the window code, as the Mac does (ADR 0153). The window code goes
   back only to the caller of `device.pair.start`; no event, snapshot or listing carries it.
2. The phone scans, then runs `pair.begin` and `pair.prove` as today (contract 08 §2.2).
3. When `pair.prove` verifies, the engine makes an **approval code**: six decimal digits from the
   system's random source, uniform (rejection sampling), unique among the pending requests. It is
   kept **in the engine's memory only**, with the request's deadline, an attempt count and nothing
   else. It is not written to the store, to a receipt, to the log or to any event. The engine sends
   it to the phone, in the answer to `pair.prove`, as a new optional field `approvalCode`, added to
   the answer after the stored result is made (as `device.pair.start` adds `reachability`), so a
   replay of the call on a lost response gets the same code from memory and the stored record never
   holds it. If the engine restarted and the code is gone, a replay of `pair.prove` that verifies
   makes a new one.
4. The phone shows the code, large (`482 913`), and under it the comparison code of ADR 0152. It
   says: type this code in Soyeht on your computer.
5. The desktop app shows the request with the device's name, the comparison code and a field for the
   code. It never receives the approval code. The person looks at the phone, finds the request whose
   comparison code is the same, and types the six digits.
6. The app calls `device.pair.decide {requestId, approve: true, approvalCode}`. In `PersonsCode`
   mode the engine approves only if all of these hold: the connection is the local human (a paired
   device, an agent and a chief are refused as before); the request is pending and its window is
   active (as today); the request has a code in memory, not past its deadline and with fewer than
   three wrong attempts; and the submitted digits equal it (a constant-time comparison). A missing
   or wrong code answers the new error code `pairing_code_invalid` and does not approve. The third
   wrong code **denies the request**: the phone gets `pair.result` with `approved: false`, and the
   person starts again. The code is single use: it is dropped when it is used, when the request is
   decided and when it expires. The check runs before the call is recorded, so a wrong code leaves
   no receipt, and the receipt of a right one comes after the code is spent.
7. `device.pair.decide` with `approve: false` needs no code in either mode (denying is the safe
   direction), and it needs the local human in `PersonsCode` mode as approving does.

Why the person is in the path: the code is in the engine and on the phone and nowhere else. A program
on the computer cannot read it from the engine (the API never sends it), from a file (never
written), from the screen (the computer never shows it), or from the wire (it travels inside the
phone's tailnet connection, which only the phone and the engine end). Only the person's eyes and
hands carry it from the phone to the app.

### The new device is not this computer

In `PersonsCode` mode the engine refuses `pair.begin` (`unauthorized`, with a message that says why)
when:

- **the peer address is this computer's own**: loopback, or any address of an interface of this
  machine (the engine already lists them for the tailnet follower; the check uses the same list,
  refreshed with it). The peer address comes from the accepted TCP connection and is recorded on
  the connection (`Connected` gets the peer address of a remote client; the local endpoint has
  none). A program here that wants to be the device has to come from another machine.
- **the peer address is the address a paired, not revoked device last connected from**. The store
  keeps one address per device, updated on each authenticated `hello` over TCP (a small migration).
  A stolen phone that is paired cannot pair "a second device" from itself, by the Soyeht app or by
  any other program on the phone, because all of its traffic comes from its node's tailnet address.
  A person who lost the credential on their phone (reinstalled the app) revokes the old record in
  the desktop app first; the refusal says so.

Tests and the headless emulator run (`docs/apps-android.md`) reach the engine from loopback; they
use a second `Hooks` method (`pairing_from_this_computer`, default false) that only
`soyeht-engine-test` implements (ADR 0091).

### Start and revoke in `PersonsCode` mode

| Call | `SignedApp` (macOS) | `PersonsCode` (Linux; Windows for now) |
|---|---|---|
| `device.pair.start` | the signed app | the local human |
| `device.pair.decide`, approve | the signed app | the local human, with the person's code |
| `device.pair.decide`, deny | the signed app | the local human |
| `device.revoke` of another device | the signed app | the local human |
| `device.revoke` of itself | the paired device | the paired device |
| any of the above | a paired device (other than its own revocation), an agent, a chief: `forbidden_role` | the same |

`device.pair.start` is open to the local human here because a window alone no longer lets a program
put an approval in front of the person that they can complete: ADR 0163's reasoning was that a
prompt with a name of the attacker's choosing invites a tap, and a tap is not enough any more. A
window opened by a program has a link that only a device on another machine can use, and nothing
the person can answer, since the code is on that other machine's screen.

`device.revoke` needs no code. There is no code to ask for: the device being revoked is the one that
shows it, and a lost phone shows nothing. Making revocation hard would also be the wrong direction,
because it is what the person reaches for when a phone is stolen. The price is stated below.

### Copy

Phone (iPhone and iPad; `PairingSession` reads `approvalCode`; when the answer has none, which is
every pairing with a Mac, the phone shows what it shows today). The strings go in the `texts` of
`x-clientPolicy`, which the Rust and Swift clients both read; the exact keys are the contract slice's:

- Title: "Type this code on your computer".
- Under the code: "Open Soyeht on your computer, choose Pair a phone and type the code there. The
  computer shows the same code beside the request: E3B0 C442 98FC 1C14. If it shows another one, or
  none, tap Cancel."
- After the third wrong code: "The code was not accepted. Start again on the computer."

Desktop (the Windows and Linux app; the view is the same on both and follows the capability, so it
needs no change when Windows is attested):

- The sheet: "Pair a phone". "Open Soyeht on your iPhone or iPad and scan this code. Both devices
  must be on the same tailnet." The QR, a "Copy link" action (a phone with no camera reads the
  link), the time left.
- A request: "Allow “<name>”?" the comparison code, "Type the code your phone shows." and a
  six-digit field. Deny beside it. Under it, one fixed sentence: "Type a code only if your own phone
  is showing it now, for a pairing you started. Nobody else will ask for it."
- A wrong code: "That code is not right. N tries left." The last one: "Too many wrong codes. The
  request was denied."
- A request that arrives while the person's pairing sheet is closed is not put in front of them as a
  question; it waits in the device list as "Waiting for approval" with Deny.
- Devices: the list with Revoke and the confirmation of the Mac ("Revoke this device's access?").

### The Mac

Nothing changes: the engine runs in `SignedApp` mode, sends no `approvalCode`, offers no
`pairing-code` capability, and `PairApprovalView` is untouched. A new phone pairing with a Mac sees
today's screen because the answer carries no code.

## What this stops, and what it does not

Stops, on Linux:

- A same-user program with no other machine: it cannot be the device (the address rule), so there is
  no request for it to approve; it cannot read the code of the person's own phone, and approving
  that one needs the code. Guessing it is three tries in a million, and approving the person's own
  request is harmless. This holds against a program that can read the screen and inject input on
  X11 too, because the code is not on the computer's screen.
- A stolen paired phone and the terminal it opened: the program in the terminal cannot pair a device
  from the phone itself (the second address rule), from the computer (the first) or alone.
- A malicious new device and a tailnet attacker that has the window code: the prompt is not
  completable without the code that its own screen shows. A tap does not approve; only typing the
  code does, and only the phone that is pairing has it.
- A phishing prompt: a window that a program opened has nothing for the person to type.

Does not stop (and the Mac does not stop the first):

1. **A program that edits the engine's database or the user's files**, the same as ADR 0128: it can
   add a device row with a credential it made. The person's account can do anything to its own data.
2. **A same-user program with an accomplice**: another machine on the owner's tailnet that is not a
   paired device's address. The program opens the window, the accomplice pairs and tells it the code
   (the accomplice's screen shows it), the program approves. The attacker needs a node in the owner's
   tailnet, which is the cost this design adds. The Mac stops this one, because a program is never
   the signed app. This is the real gap between the two systems.
3. **A program that revokes the owner's other devices**: a denial of remote control, not a gain of
   power. The person at the computer can pair again, and the program cannot stop that.
4. **A program that denies or burns the attempts of a request**, or opens windows to replace the
   person's: the person starts again. A same-user program can already stop the engine.
5. **Root**, or a debugger in the engine, which can read the code from memory. The engine marks
   itself non-dumpable on Linux (`soyeht_platform`, one call; `PR_SET_DUMPABLE`), which with the
   usual `ptrace_scope` stops a same-user process from reading it, but the design does not rely on
   it: the code is useless without a second machine to be the device.
6. **A person who types a code an attacker dictates** over the phone or a message. The sentence on
   the desktop sheet says not to.

The code protects the engine's API and the person's own taps; it does not make a computer safe from
a program running as its user (ADR 0128).

## Compatibility and the protocol

All changes are additive (ADR 0161): a new optional `approvalCode` in the answer of `pair.prove` and
in the parameters of `device.pair.decide`, a new `EngineCapability` (`pairing-code`), a new error
code `pairing_code_invalid` (`ErrorKind` is an open enum), the `texts`, and the refusal of
`pair.begin` in two new cases (an existing `unauthorized`). Nothing is removed or made required; a
`decide` without the field still works in `SignedApp` mode. If this lands before the contract of the
first release is snapshotted (`contracts/released/`), it is part of version 1; after that, `current`
rises to 2 with `min` still 1. The pairing dialogue has no `hello`, so the presence of the field is
the signal: an old phone with a Linux engine never shows the code and the person cannot approve it
(the desktop sheet says: update Soyeht on the phone); a new phone with a Mac sees no field and shows
the old screen.

A vector file, `contracts/vectors/pairing-approval.json`, holds the code's shape (six ASCII digits,
the grouping `482 913`, the digits accepted when typed with a space) for both clients.

## Rejected and kept in reserve

- **The path of the installed executable** (ADR 0162's first option): raises a cost only, and does
  not hold for an install the user can write.
- **Variant B: the computer shows the code and the person types it on the phone.** The engine would
  have to send the code to the app, and a program on the computer can ask for it as the app does;
  on X11 it can also read it from the screen. Variant A keeps the code off the computer.
- **The ancestry of the peer** (refuse a local connection whose process descends from the
  supervisor): stops a plain child of a pane, and a double fork gets out of it. It may be added as a
  cheap extra; nothing relies on it.
- **The account password through polkit**: the one thing no same-user program can fake, and it would
  also stop item 2 above. Not the base because it needs a policy file that a package installs and an
  AppImage cannot, a polkit agent in the session, a bus client in the engine, and a password at every
  pairing. It stays the answer if the owner wants a guarantee equal to the Mac's.

## Defaults taken, each easy to change

1. Six digits and three attempts. The only party that can guess is a program that cannot be a
   device, so the code needs to resist a person typing, not an offline attack.
2. The code lives until the request's deadline (at most the window's ten minutes) and not past an
   engine restart.
3. Revoke and deny need no code; start needs only the local human.
4. The second address rule keeps one address per device in the store, and refuses a device whose
   address belongs to a paired one. A "same node" check by the tailnet's node identity, which also
   covers a node with two addresses, is a later hardening.
5. Android keeps its core (the shared Rust pairing code carries the new field and an event for the
   code) but its screen waits for its own release.

## Slices

| # | Slice | Size |
|---|---|---|
| 1 | Contract: `approvalCode` (answer and parameters), capability, error code, `texts`, the vector file, generated code, contract 08 §2.2 and §2.4, the status lines of ADR 0128, 0162 and 0163, README and `docs/platforms.md` | S, 0.5 day |
| 2 | Engine, the mode and the code: `pairing::Approval`, the in-memory codes and attempts, the three-method table, `pairing_code_invalid`, the `Hooks` method, non-dumpable on Linux | M, 2 days |
| 3 | Engine, the address rules: the peer address on `Connected`, the local-address check, the store migration for the device address, the second hook | M, 1.5 days |
| 4 | Desktop pairing view: the sheet, the QR (the desktop has no encoder; a permissive-licence crate goes through the dependency review of ADR 0149), the request row, the field, the device list and Revoke, `soyeht-client` calls and a `Progress` event for the code in `device::pair` | L, 4 days |
| 5 | Shared Swift and iOS: `PairingSession` reads the code, `PairingCopy` and the view show it; the Android core passes the field through | S, 1 day |
| 6 | Tests and docs (below), `docs/agent-acceptances.md`, `docs/dev-local.md` | M, 1.5 days, alongside 2 to 5 |

Slices 1 to 3 are one engine change and need the second-agent review (pairing, protocol, store,
dependencies). The order is 1, 2, 3, then 4 and 5 in parallel.

Tests:

- Engine unit: the table of the three methods by role, by mode (through the hook), with and without
  the code; wrong code, three wrong codes, expiry, single use, restart, the replay of `pair.prove`.
- **A local socket client without the person's code is refused**, in `attack_pairing.rs`: a client
  that is the local human but not the app opens a window, cannot decide with no code, a wrong code or
  a made-up one, and a device paired from this computer's address is refused at `pair.begin`. It runs
  in the macOS gate through the hook and natively on Linux (`scripts/check_engine_linux.sh`, with
  `scripts/vmpal_check_engine.sh`), where it also asserts that the mode is `PersonsCode` with no hook.
- No leak: a whole pairing, then a scan of every frame that a local connection received, the
  database file and the log for the approval code.
- Address rules: loopback and an interface address refused; a paired device's address refused;
  allowed again after its revocation; the refusal text.
- Desktop: the pairing state machine against a scripted engine (window, request, wrong code, right
  code, denial, expiry). Swift: `PairingSession` with and without the field, the vector file, the
  copy. Contract: `generate.py --check`, `check_doc_names.py`.
