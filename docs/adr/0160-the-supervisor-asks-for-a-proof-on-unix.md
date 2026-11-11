# ADR 0160: The supervisor asks for a proof on Unix

- Status: accepted. Extends ADR 0003 (the supervisor channel) and ADR 0086 (the local endpoint).
- Date: 2026-11-10

## Context

The supervisor (`soyeht-ptyd`) starts the programs of the panes and holds their terminals. After
`hello` a caller can spawn any command, write to any terminal and read any terminal. On Windows
the connection first proves that it knows a secret that only a process able to read an owner-only
file has (contract 08, section 2.5). On Unix the only protection was the socket's mode and the
check that the peer runs as the same user, so any process of the user that could reach the socket
could take the supervisor over. An audit found two faults in `engine/soyeht-ptyd/src/server.rs`:

- `hello` carries a protocol version and the engine generation and no secret.
- A `hello` with a higher generation closes the live engine connection and the supervisor saves
  that generation to its registry. The real engine, which takes the next number of its database
  at each start, is then refused as stale at every start, until someone edits the registry file.
  One message of `u64::MAX` was enough.

## Decision

**A socket can ask for the proof; the supervisor's does.** The exchange of Windows (a random nonce
from each side and an HMAC of both under the secret, the secret never sent, the client sending only
its nonce until the listener has proved it knows the secret) now runs over a Unix socket too, and
it is the same code: `endpoint::serve` and `endpoint::prove` in `soyeht-platform` are written over
a small `Proving` trait that the loopback stream and the Unix stream both implement, and the
accepting side (`handshake::Accepting`, which examines each connection on a thread of its own,
bounds those in flight and drops the oldest at the limit) is shared by both. The bytes are those of
section 2.5 of contract 08. What differs is only where the secret lives: the loopback endpoint
keeps it in its endpoint file, a Unix socket keeps it in an owner-only file beside the socket,
`ptyd.sock.secret`, one line `soyeht-local-2 secret <64 hexadecimal digits>`.

The API has the same shape on every system: `local::ProvedListener`, `local::connect_proved` and
`local::remove_proved`. On Windows they are the listener, the connection and the file removal of
the loopback endpoint, which always asked. On Unix `ProvedListener::bind` makes a new secret,
writes it with `fs::write_atomic` in `WriteMode::OwnerOnly` (so it is owner-only from its first
byte), then binds the socket; `accept` yields only connections that proved. The supervisor binds
a `ProvedListener`; `Client::connect` and `Watcher::connect` use `connect_proved`. The proof is
the first thing on the connection: the supervisor reads no frame of its own protocol, so no `hello`
reaches `connection`, the registry or the live connection from a peer that did not prove. A wrong
proof is dropped with one `local.proof_refused` log line (a peer that hangs up, as the probes that
ask whether a supervisor runs do, is not logged).

I put the proof under `hello`, in front of it, and not inside it. A proof in `hello` needs a
challenge before the answer, which is three messages in a protocol whose first message is the
`hello`, a second request type for the supervisor to handle in a state it has not authenticated,
and a second implementation next to the Windows one. In front of the protocol it is one function.

**The secret is replaced at each supervisor start, and read at each connection.** The supervisor
holds its singleton lock first, then writes the new secret, then binds the socket. A client connects
first and reads the secret second, so one that meets a supervisor that has just restarted reads
the secret that the new one wrote before it bound. The client opens the file without following a link, refuses one that is not
owner-only (`file_is_owner_only`), as one that others can read or write says nothing about who holds
the secret, and reads no more than the file's one line and a little slack. The supervisor removes the socket and the secret file when it stops; one that is killed
leaves them and the next replaces both. A socket with no secret file beside it is a supervisor from
before this change: `connect_proved` fails with `InvalidData`, not `NotFound`, so the engine does
not read it as an absent supervisor and start a second one.

**A hello has no upper bound on the generation.** A hello (of either role) whose generation is
lower than the recorded one is refused with `stale_engine_generation`, before anything is closed
or saved (`hello_refusal`, which reads the registry and never changes it, so a refused hello of any
kind leaves the registry, the live engine connection and the watches as they were). One that is
higher takes over, however far. I first bounded the step (16 past the recorded number) and took it
out: the engine does not count connections. `next_engine_generation` adds one to a number in its
database early in `Core::open_with`, and the supervisor learns the number only when the engine
connects to it. An engine that crashes between the two (a restore error, running out of memory)
and is restarted by the service manager every second burns a number per start, so the next good
engine can be any distance ahead. A bound makes that engine unreachable for good, until someone
deletes `ptyd-registry.json`, which is the lockout the bound was meant to prevent. It bought almost
nothing in return: a peer that has not proved the secret never reaches `hello`, and one that has
can read the engine's database and signal the supervisor anyway. The proof, and the stale check
that stops an old engine from taking the supervisor back, stay. A holder of the secret can still
raise the recorded number past what the real engine will present; that is inside the strength the
proof claims (below), and recovery is the same as before: stop the supervisor and delete
`ptyd-registry.json`, which costs nothing the supervisor keeps, since it marks every process of a
previous start as closed when it loads the registry.

**The protocol version is 6.** An engine of this build cannot talk to a supervisor of the previous
one (no secret file) and the other way round, so `soyeht_ptyd::wire::VERSION` goes from 5 to 6.
ADR 0122 says a release that changes the supervisor's protocol counts as a supervisor change, and
the Windows installer compares `soyeht-ptyd --protocol-version`, which prints this number
(ADR 0123); with the bump both treat this release as one that restarts the supervisor and ends the
terminals, after the person confirms.

## What the proof stops, and what it does not

It stops a process of the user that cannot read the data directory's files: an app inside a sandbox
that is not given the folder, a process in a container or under another confinement, an exploited
program that is limited to what it was given. It does not stop a process that runs as the user
without confinement, which can read the secret file as the engine does and can also read the
engine's database, signal the supervisor or attach to it. That is the strength the Windows endpoint
has had since ADR 0086; a boundary against the user's own unconfined processes is not claimed.

## Alternatives

- **The proof in `hello`.** Rejected above: three messages and a second implementation.
- **Mutual TLS or a key pair on a socket.** A certificate needs a place to keep it, which is the
  same owner-only file, and a TLS stack for a socket the kernel already makes private.
- **Reading the peer's executable (the code signature of the connecting process).** It would stop
  the unconfined process too on the Mac, and is no use on Linux or Windows, which need the same
  rule. It stays a question for the signed-app check that the engine already makes for some methods
  (ADR 0128 and contract 08, section 2).
- **A bound on the step.** Rejected above: a crash loop of the engine before it reaches the
  supervisor can leave any distance, so a bound shuts the next good engine out.

## Consequences

- A supervisor started before this change (protocol version 5) cannot be reached by an engine of
  this one (its socket has no secret file), and the other way round (the first bytes of the
  exchange are not a frame).
  A supervisor is replaced only by a restart, which ends its terminals, so the first release that
  carries this one is also the first that needs that restart. Contract 07 already says an update of
  the supervisor warns that it stops the agents.
- A new engine always meets a supervisor of its own build in a fresh data directory, which is the
  case for everything not yet released.
- Every client of `ptyd.sock` proves: the engine, the end-to-end scripts (`connect_endpoint(...,
  proved=True)`, which reads the secret file the same way) and the tests. A probe that only asks
  whether something listens (`connect`) still works, and costs the supervisor one thread for as
  long as the peer stays silent, at most the 2 seconds of the exchange.
- The end-to-end cleanup and the engine test rig, which closed processes with a generation of
  `u64::MAX` (which would now be accepted and would stale the real engine), take the recorded
  generation plus one.
- `hmac`, `sha2` and `getrandom` become dependencies of `soyeht-platform` on every system; they
  were its dependencies on Windows and in its tests.
