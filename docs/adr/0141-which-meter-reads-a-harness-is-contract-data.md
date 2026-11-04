# ADR 0141: Which meter reads a harness is contract data, and the Android pane header draws it

- Status: accepted
- Date: 2026-11-04

## Context

The Mac and the iPhone mark the harness chip of an agent composer with the logo of the provider whose usage
meter reads that harness (`UsageProvider(harnessId:)`: the provider names Claude, Codex or Cursor, found in the
harness id). It was a rule in one Swift file. The Android app wants the usage ring of that provider in the
header of an agent pane, and the desktop client has `usage::Presentation`, which draws the ring's state for
Mac, iPhone and desktop alike. A second spelling of the rule in Rust would be a second copy, and OpenCode's
meter is about to follow the provider OpenCode is set to (ADR 0133), so the rule has to be able to change.

## Decision

- **The rule is `usageHarnesses` in `x-clientPolicy`**: for each provider, the lower-case text its harness id
  holds. The generated forms are `UsageProvider::for_harness` (Rust) and `UsageProvider(harnessId:)` (Swift),
  which replaces the hand-written initializer; `contracts/vectors/usage.json` has the cases (`harnessProviders`)
  and both clients pass them. OpenCode is in the table, so its harness now has a mark and a ring.
- **A provider can be chosen at run time.** `soyeht_client::usage::HarnessProviders` answers `provider_of(harness)`:
  a provider given to that exact harness id while the app runs, else the contract's rule. The Android core
  keeps one and exports `choose_usage_provider`; a client that learns which provider OpenCode is set to gives it
  there, and no table is edited.
- **The ring is the core's, the drawing Kotlin's.** The core exports `usage_gauge(pane_id)`: the reading of the
  chosen provider through `usage::Presentation` (the window the ring follows, the band from the thresholds of
  `x-clientPolicy`, the opacities of a last-known or missing reading, the percentage). `load_usage` asks the
  engine for the readings. `UsageRingView` draws the track and the arc from the `UsageRing` tokens, in the
  band's palette roles (the ANSI red, green and yellow slots), inset by half the widest line as the other
  clients do. A pane whose harness no meter reads, or whose provider has no reading yet, shows no ring.

## Not done

The detail the Mac shows on hover (windows, reset times, the source) and the list of every provider the Mac
and iPhone Settings show.
