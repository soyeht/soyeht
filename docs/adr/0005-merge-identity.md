# 0005 — Identity recorded on merge

Status: documented limitation, 2026-09-11.

Local commits use the author and committer configured on the machine:
Caio Salgado. Messages are checked before pushing and receive no
attribution trailers.

Authorization requires squash merging on the platform. That service records
its own committer and uses the authenticated account's identity as author.
The API refused an attempt to select the email configured in Git with
`Invalid email address`. Neither the account nor its configuration was changed.

Merges continue through the already authenticated account, without direct pushes
or rewriting main. The difference between local commit and squash identities
will be disclosed in the final report. Standardizing the squash author's email
requires that address to be accepted by the account on the platform.
