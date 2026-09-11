# ADR 0009 — Client metadata updates

Status: accepted for slice 1.

Resizing a pane and changing its recording alter the snapshot. The protocol
scope did not name an event to publish those changes. The engine uses the
additive extension `pane.updated {pane}`, persisted in the same transaction.
This keeps other clients current without requesting another full snapshot.
`settings.changed {key, value}` does the same for settings.

The final snapshot result includes `focus {workspaceId?, paneId?}`. This focus
belongs to the authenticated client's identity. Reconnecting the Mac restores
its focus without applying the focus chosen on the phone. Closing a pane clears
references to it in the same transaction.

The `requests` section includes the `DevicePairPending` type, with device name
and fingerprint, request identity and deadline. The proof, code and token never
appear in this section. The new fields and events live in the single schema
and have vectors for all three generated consumers.
