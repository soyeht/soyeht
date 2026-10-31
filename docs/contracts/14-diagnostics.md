# Contract 14 · The diagnostics report

Status: v1. Decision: ADR 0118 (the report), amended for the clients' "Copy diagnostics" action.
Code: `soyeht_platform::diagnostics` (the report and the redaction), used through
`soyeht diagnostics`.

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
| Crash reports | the crash reports the programs wrote (contract 16), under `### Crash reports`, or `(none)` | `crashes/` of the data directory |

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
- the value of a field whose name ends in a secret word: `token`, `secret`, `password`, `passwd`,
  `passphrase`, `authorization`, `apikey`, `cookie`, `credential(s)`, `bearer`, `pairing`,
  `session`, `private`, `pin`, `otp` or `key` (`"authToken":"..."`, `x-api-key: ...`,
  `password=...`, `Authorization: Bearer ...`, `"pairingCode":"482913"`), and `code` only after
  `pairing`, `otp`, `verification`, `confirmation`, `security`, `login`, `auth`, `activation` or
  `invite`. Words are split at `_`, `-` and capitals, and the secret word must be the last one, so
  `exitCode`, `sessionCount`, `keyboard`, `mapping` and `monkey` stay. In running text, the
  number of four digits or more after `pin`, `otp` or `pairing`, with or without `code` (`pairing
  code 482913`), and after `verification code` and the like, goes too;
- any run of 24 characters or more that mixes letters with digits, or upper with lower case. A
  UUID or a ULID standing alone (the ids of panes and workspaces) is kept, so an issue can tell
  panes apart, unless a field named like a secret holds it or it follows a key prefix such as
  `sk-`, `pk-` or `ghp_`; a name in camel case
  (`ConnectionRefusedByTheServer`) is kept. A ULID-shaped random key is the one token this keeps.

A text is cut at 2,400 characters before it is read, and what is cut is dropped, never shown, so
a huge line (a 200,000-character one is read in well under a second) cannot hold the command. An address
may follow `_` or `-` (`peer_100.64.12.7`). A JSON escape of a line break or tab (`\n`, `\t`)
before a path ends the text before it.

It errs toward removing: text that is harmless can be lost. Event names
(`engine.reconnect_attempts_exhausted`) and failure kinds (`ConnectionRefused`) stay. A line is cut
at 600 characters. The pass over a line is linear in its length.

What it does not remove, on purpose or for want of a rule:

- a domain name that does not end in a local or tailnet suffix (`example.org`), unless it is a
  link or part of an address;
- a relative path with a single separator (`src/main.rs`, `Client/x.rs`), since a word with a
  slash is too common to cut, and a path that starts with a bare word (`Projects`);
- a quote inside a quoted path: a path in a JSON value ends at its first unescaped quote, so a
  folder name with a quote in it leaks the rest;
- a random key that is shaped like a ULID or a UUID and stands alone, and a secret of fewer than
  24 characters in a field whose name is not a secret word;
- the words of a passphrase after its first.

## 3. Rules

- The report is built in one place, `soyeht_platform::diagnostics::report`, and gathered by one
  program, `soyeht diagnostics`. A client that shows "Copy diagnostics" runs that program (section
  4) and adds no fact of its own.
- Nothing in the report may be a value typed in the code: the version comes from the build, the
  profile and the log names from the contracts.
- A failure to read a log is said by its kind (`unreadable: PermissionDenied`), never by the
  message, which can carry a path.

## 4. Copy diagnostics in the clients

The Mac app and the Windows and Linux app each have one action, "Copy diagnostics", that puts the
report of section 1 on the clipboard so a person can paste it into a support message. It is in
Preferences (the Setup page on the Windows and Linux app) and, while the engine does not answer, in
the area that says so (the setup list of the empty workspace on the Mac, the welcome screen on
Windows and Linux).

**The seam is the program, not a library call and not an engine method.** Each client runs the
`soyeht` program that ships with it, `soyeht --profile <profile> --data-dir <data directory>
diagnostics`, and copies what it printed (standard output, UTF-8 Markdown) as it is:

- The Mac runs the `soyeht` of its bundle (`Contents/MacOS`), through the same runner it uses for
  `soyeht service` (ADR 0120). A build whose bundle carries no `soyeht` (a Dev build, or any
  profile packaged without the programs) runs the copy in the profile's `bin` folder when there is
  one, which is where the Dev installer puts it, and otherwise says there is no program.
- The Windows and Linux app runs the `soyeht` that sits in the folder of its own executable, where
  the engine and the supervisor are (ADR 0102), through `soyeht_process::run`.
- Both clients bound the program the same way, with the values of `x-clientPolicy`: it ends after
  `timeouts.diagnosticsMs` (the runner kills it and what it started), and a report larger than
  `caps.diagnosticsReportBytes` is a failure, not a truncated copy. The Mac's runner stops reading
  and ends the program at the cap, as the Windows and Linux runner does.
- The profile and the data directory are the ones the client itself resolved, so the report is
  about the profile's own data directory. When a window was pointed at another engine, with
  `SOYEHT_ENGINE_SOCKET` (a developer or test setting), the report still covers the profile's own
  directory and says nothing of that engine.

Why the program and not a call to `soyeht_platform::diagnostics::report`: the report needs more
than the builder, namely whether the two endpoints answer, the readiness question to an engine
that does, and the list of log files. That gathering is `soyeht-cli`'s, and a client that called
the builder would copy it, in Swift as well as in Rust. Running the program gives both clients the
same text from the same code, the case that matters (an engine that is down) included, and no
client holds a redaction rule. The cost is that the client must find the program beside it; a
build that does not carry it says so (below) instead of making up a report.

The client adds nothing to the text and removes nothing from it, and it does not show it: the
report goes to the clipboard only, since the person reads it in the support message they paste it
in. A failure is never silent. The client says one sentence, and logs the kind of the failure and
never its message (the program's error text can carry a path, which the report's redaction would
have removed):

| Case | What the person reads |
|---|---|
| Collecting (the action is disabled meanwhile) | `diagnosticsWorking` |
| The report is on the clipboard | `diagnosticsCopied` |
| The program ran and failed, did not finish in time, or printed more than the cap or something that is not text | `diagnosticsFailed` |
| The client cannot find the program | `diagnosticsUnavailable` |

The labels and sentences are the `diagnostics*` entries of `x-clientPolicy.texts` in
`contracts/protocol.json` (`diagnosticsTitle`, `diagnosticsSummary`, `diagnosticsAction`, and the
four above), which both clients read through their generated forms.

## 5. Not covered

A new engine method for a client that cannot run the program (a web client) is a follow-up. The
report does not say whether the services are registered with the system (that needs the service
manager's own tool); `soyeht service status` does.
