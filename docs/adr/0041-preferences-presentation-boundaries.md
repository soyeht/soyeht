# ADR 0041 · Preferences presentation boundaries

Date: 2026-09-12
Status: accepted

## Context

The approved preferences frame includes a harness registry action, harness-level
default selectors, raw-terminal heuristics, and navigation to preference pages
that do not have protocol methods or persisted settings in slices 1–3.

The design-alignment pass cannot add protocol, engine, or store behavior. It
also cannot present a control as usable when the app cannot persist its result.

## Decision

The application command opens the approved preferences panel over the current
workspace. The Harnesses page reads the live harness catalogue and probe facts
from the engine. Its row status and identity are live data.

Registry installation, harness defaults, raw-terminal heuristics, and the
remaining sidebar destinations are visual previews. They have no action and are
hidden from accessibility as controls. A later slice must either add contracted
bindings for them or remove those affordances from the approved design.

The panel closes with Escape or by clicking outside it. This supplies a real
way out without adding a control absent from the approved frame.

## Consequences

The screen matches the approved frame while preserving the behavior boundary
of the first three slices. It does not imply that a displayed preview changes
engine configuration.
