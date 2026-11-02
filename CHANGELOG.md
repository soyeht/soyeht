# Changelog

All notable changes to Soyeht are written here, for the person who uses it. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the version is the one in
`contracts/release.json` ([ADR 0115](docs/adr/0115-one-product-version.md)).

A release's section is drafted from the pull requests merged since the last tag
(`uv run python scripts/changelog.py draft`), edited by a person, and becomes the text of the
GitHub release that the update notice links to (`docs/release.md`). Entries that wait for the next
release go under Unreleased, in the categories Added, Changed, Deprecated, Removed, Fixed and
Security.

## [Unreleased]

### Security

- A paired iPhone or iPad now checks that the server it connects to is your Mac. The Mac signs each
  connection with a key that the pairing code carries. **Every iPhone and iPad paired before this
  version must be paired again**, and the Mac must be updated before the phone is: a phone that finds
  an older Mac says to update it, and one paired before says to pair again.

