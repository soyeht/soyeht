# Security policy

## Reporting a vulnerability

Please do not open a public issue or pull request for a security problem.

Report it privately, by either channel:

- **Preferred:** GitHub private vulnerability reporting. On the repository page, open the
  **Security** tab and choose **Report a vulnerability**.
- By email to **caio.salgado@hotmail.com**, if you cannot use GitHub.

Include what you can of:

- the component and commit (`git rev-parse HEAD`) and the system you ran it on;
- the steps to reproduce, or a proof of concept, kept as small as possible;
- what an attacker gains and what they need first (a local account, a paired device,
  network access to the tailnet);
- whether the problem is already public.

Do not include real credentials, tokens or other people's data. If you are unsure whether
something is a vulnerability, report it anyway.

## What to expect

- We acknowledge your report within 3 business days.
- We give you an assessment (whether we consider it a vulnerability, and how severe) within
  10 business days of the report.
- We keep you informed while we fix it, and agree a disclosure date with you. We credit you in
  the fix unless you prefer not to be named.

Nothing here is a bounty offer.

## What is in scope

Soyeht is early software with no release yet; the supported code is the `main` branch. These
parts are security-sensitive, and a problem in any of them is in scope:

- **The engine's local endpoint.** The Unix socket (mode 0600 in a 0700 data directory) and,
  on Windows, the loopback endpoint guarded by a secret in an owner-only file. Anything that
  lets another OS user, or a process that should not have the right, talk to the engine, read
  or write the data directory, or follow a link out of it. The declared limit is that any
  process of the same OS user can use the local socket (`docs/contracts/08-engine-and-clients.md`,
  section 2).
- **Pairing and device authentication.** The pairing code and consent flow, device tokens
  and the proof of possession over the connection challenge, revocation, and the roles
  (`human_local`, `human_device`, `agent`, `chief`) that decide who may call a method. The
  engine also listens on the loopback and tailnet addresses (port 8291 for the Dev profile, 8290 for the
  production profile) for paired devices.
- **Web permissions.** The web pane's session permission, which an agent permission mode must
  never answer, page evaluation, cookies and incognito sessions, and the host-routing rules
  (`docs/contracts/11-browser.md`).
- **Agent launch and consent.** Which command a harness may start (custom harnesses are a
  local file the remote side cannot supply), the permission requests an agent raises and who
  may answer them, and the consent-changing methods that only a signed local UI may call.
- **The process supervisor** (`soyeht-ptyd`), push notifications, and the skills and MCP
  server that agents are given.

## What is out of scope

- A process running as the same OS user that already controls everything that user owns.
- Weaknesses in the third-party agent harnesses or services that Soyeht launches, and in the
  tailnet itself. Report those to their owners; do tell us if Soyeht makes them worse.
- Findings that need a modified build, a debugger attached to the engine, or the test hooks
  (`soyeht-engine-test`, `test-hooks`), which no shipped build contains.
- Denial of service by a user of their own machine, and missing hardening of development
  scripts under `scripts/`.

## Supported versions

Only the latest commit of `main` until the first release.
