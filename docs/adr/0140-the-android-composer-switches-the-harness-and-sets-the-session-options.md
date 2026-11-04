# ADR 0140: The Android composer switches the harness and sets the session options

- Status: accepted
- Date: 2026-11-04

## Context

The iPhone's agent composer names the harness the pane runs ("Claude") on a menu that restarts the pane on
another harness and hands the conversation over, and lists the model, effort and permission menus of the
session (`PhoneHarnessMenu`, `AgentComposerControlViews`). Which selector is the model, the effort or the
permission one is `ConfigOption::is_model`, `is_effort` and `is_permission_mode` (`x-clientPolicy`), how
harnesses are grouped and when a switch may start are `contracts/vectors/harness-picker.json` and
`harness-switch.json`, and the desktop client already holds all of it (`soyeht_client::agent::Controls`,
`harness_picker`, `switch`). The Android app had a message field and nothing else.

## Decision

- **The core answers, the view draws.** The core exports `composer(pane_id)`: the harness chip (the title the
  engine's catalogue gives, "Claude", or the harness a running switch goes to; whether the menu may open) and
  the model, effort and permission menus, each with its choices and its current one. `harness_choices(pane_id)`
  lays the other harnesses out as the new-agent picker does: groups by how ready they are, those not installed
  under one "More harnesses" entry, the running harness left out. Nothing is typed in Kotlin: not a method name,
  not a selector id, not a group title, not the sentence that says what a switch does to the conversation, and not "Loading agents…",
  "No other agents", "Check agents again" or "Cancel": those are `x-clientPolicy` texts (`harnessesLoading`,
  `harnessesNone`, `harnessesRescan`, `cancel`), which the core hands over with the choices. The desktop's
  harness menu reads the first of them too; the Mac and the iPhone still hold their own copies of these
  strings, which they may drop for `ClientPolicy` when next touched.
- **When a switch may go to a harness is one function.** `switch::startable(harnesses, running, target)` in
  `soyeht-client`: the engine lists the harness as able to start and it is not the one the pane runs. The core
  and the desktop's switch popover (`switch_flow.rs`) both call it.
- **Commands are the client's.** `load_harnesses`, `choose_option` and `switch_harness` send the
  `soyeht_client::Command`s the desktop sends. A switch the view lets through by mistake is not sent:
  `switch_harness` asks the same `switch::can_switch` (connected, no configuration change in flight, no switch
  running) and sends only to a harness the engine lists as able to start and that is not the one running.
- **The shared vector is passed.** `composer::tests::the_chip_follows_the_shared_switch_vector` runs every case
  of `contracts/vectors/harness-switch.json` through the chip's `can_switch`, with the engine's events
  producing each phase of a switch, as the Mac and the desktop do.
- **The view.** A row of flat chips scrolls sideways above the message field: harness, model, effort,
  permission. Each opens a menu; the current choice carries a check. The harness menu asks the person to
  confirm ("Restart conversation with Codex", and the handover sentence, which a menu would cut short) before
  anything is sent, and offers "Check agents again" (`rescan`). The menus wait while a configuration change is
  applied. A failed switch shows as the pane's notice, as it does on the other clients. The pane's title follows
  the computer, because a switch renames the pane.

## Not done

The account section of the iPhone's menu (sign out, switch account), and choosing a model and effort for the
harness being switched to before confirming (the desktop popover does; the phone leaves both to the harness,
as the iPhone does).
