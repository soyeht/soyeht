# Contract 14 · The diagnostics report

Status: v1. Decision: ADR 0118. Code: `soyeht_platform::diagnostics` (the report and the
redaction), used through `soyeht diagnostics`.

A person whose Soyeht breaks must be able to send us something useful without us asking for
files, and without the report leaking anything of theirs. `soyeht diagnostics` prints one
Markdown report to paste into an issue.

## 1. What the report holds

| Section | Content | Source |
|---|---|---|
| Header | product version, profile, operating system and architecture | the `soyeht` program's own version (`CARGO_PKG_VERSION`, from `contracts/release.json`), the selected profile, `std::env::consts` |
| Services | whether the engine and the supervisor answer on their endpoints | a connection to `engine.sock` and `ptyd.sock` of the data directory; nothing is started |
| Readiness | the family the engine named and, for each item, `found`, `MISSING (required)` or `missing (optional)`; no install hints | `engine.readiness` (contract 12), asked only when the engine answers; otherwise "The engine did not answer" |
| Logs | the last lines (40 by default, at most 400) of `engine.log`, `ptyd.log` and `desktop.log`, and of the file rotation made of each when the live one is short | `logs/` of the data directory (ADR 0082, 0101) |

The report holds no file path, no user name, no token and no terminal contents. The logs already
hold event names and failure kinds, never message text or terminal output (ADR 0088); the
redaction below is the second guard, for a line that carries more than it should.

## 2. What is removed from a log line

Every line of every log, and the readiness family and item ids, pass through `Redactor::redact`.
It reads the text and the person's home folder, user name and host name, so it is the same on
every system:

- the home folder, in every spelling (`/`, `\`, and the doubled `\\` of a JSON line);
- every absolute path (`/x`, `~/x`, `C:\x`, `C:/x`, `\\server\x`). A quoted JSON value ends at its
  quote. An unquoted path runs through blanks in its folder names (`My Projects`) to a quote, a
  bracket, a comma, ` -x`, or `: ` and a letter, and so may take a word after it with it. Any
  relative path of two folders or more goes too;
- every link (`scheme://...`) and every e-mail address;
- addresses: IPv4 (with its port), IPv6 (with its zone), hardware addresses, names ending in
  `.local`, `.lan`, `.ts.net`, `.localdomain`, `.internal` or `.home.arpa`, and the machine's own
  host name and its first label (read through `soyeht_platform::hostname`);
- the user's name, as a word, in any case, with a plural or possessive `s` (`Caios-Mac-Studio` for
  `caio`). A name of fewer than three characters is not replaced as a word, so other words stay
  whole (it is still removed wherever a path or an address shows it);
- the value of a field whose name holds `token`, `secret`, `password`, `passwd`, `passphrase`,
  `authorization`, `apikey`, `api_key`, `cookie`, `credential`, `bearer`, `pairing`, `session` or
  `private`, or a word `pin`, `otp`, `code` or `key` (`"pairingCode":"482913"`, `x-api-key: ...`,
  `password=...`, `Authorization: Bearer ...`), whatever the value's length. The short words must
  start a word of the name, so `mapping` and `monkey` do not match;
- any run of 24 characters or more that mixes letters with digits, or upper with lower case. A
  UUID or a ULID standing alone (the ids of panes and workspaces) is kept, so an issue can tell
  panes apart, unless a field named like a secret holds it; a name in camel case
  (`ConnectionRefusedByTheServer`) is kept. A ULID-shaped random key is the one token this keeps.

It errs toward removing: text that is harmless can be lost. Event names
(`engine.reconnect_attempts_exhausted`) and failure kinds (`ConnectionRefused`) stay. A line is cut
at 600 characters. The pass over a line is linear in its length.

## 3. Rules

- The report is built in one place, `soyeht_platform::diagnostics::report`. A client that shows
  "Copy diagnostics" calls the same function (or the command), and adds no fact of its own.
- Nothing in the report may be a value typed in the code: the version comes from the build, the
  profile and the log names from the contracts.
- A failure to read a log is said by its kind (`unreadable: PermissionDenied`), never by the
  message, which can carry a path.

## 4. Not covered

A "Copy diagnostics" action in the Mac app and in the Windows and Linux app, and a
new engine method for clients that cannot run the command, are follow-ups. The report
does not say whether the services are registered with the system (that needs the service
manager's own tool); `soyeht service status` does.
