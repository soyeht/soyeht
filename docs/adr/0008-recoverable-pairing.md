# ADR 0008 — Recoverable pairing

Status: accepted for slice 1.

The pairing code and pending requests last ten minutes. The database stores
the completion proof and approved result until that deadline. A failure
between consent on the Mac and the response to the phone therefore does not
require approving the same device twice. To recover the result, the phone
must present the proof bound to its key and challenge again.

The permanent device record contains the public key and token hash.
The temporary pairing receipt contains the token so it can be delivered
again. This receipt stays in the user's private database, with file mode 0600
and directory mode 0700, and is removed after the deadline. This does not
promise protection against another process already holding the same OS user's powers.

The snapshot includes proven requests still awaiting consent.
Reconnecting the Mac app does not hide a request received during its absence.
The code does not open the LAN: remote transport uses a tailnet address.
Without that address, the remote flow depends on making the tailnet
available before starting the engine.

Engine and supervisor generations are separate counters. The first changes
when the engine restarts; the second changes only when the supervisor restarts.
The supervisor's persistent registry keeps format 1, with the additional counter
optional when reading older files. The protocol between the two processes
moves to version 2 to include input receipts.
