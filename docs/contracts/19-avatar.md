# Contract 19 · The workspace's avatar

Status: v1, slice 1 of ADR 0166 (the library, the binding and the build request; the avatar pane, the
animation, the voice loop and the builder are later slices). Code: `soyeht_engine::avatars`,
`soyeht_store::avatars`. Limits: `avatar*` of `x-limits` in `contracts/protocol.json`.

A workspace is a project, and it has one avatar agent: a person-shaped face the person talks to by voice
or by text, thinking with the harness the person chose. This contract is what the engine stores and
serves for that: a library of avatars that every workspace can reuse, and the binding of one avatar to a
workspace with the settings that avatar needs. It adds methods, events and one table family and changes
no existing method, type or event (ADR 0161). The pictures, the speech and the animation run in the
clients and the builder; the wire carries identifiers, settings and the bytes of a picture or a package,
never audio.

## 1. What is stored

**Avatar** (the library is global to the person):

| Field | Meaning |
| --- | --- |
| `avatarId`, `name` | the identifier the engine made, and the person's name for it (1 to `avatarNameChars` characters, trimmed) |
| `kind` | `preset`, `uploaded` or `generated`: where the picture came from. The engine only records it |
| `sourceHash` | the lowercase hexadecimal SHA-256 of the source picture |
| `buildState` | `none`, `building`, `ready` or `failed` |
| `failureKind` | present with `failed`: `provider`, `frames`, `package`, `cancelled` or `interrupted` |
| `packageHash` | present with `ready`: the SHA-256 of the package's manifest |
| `voice` | the default `AvatarVoice {providerId, voiceId}` of the avatar, when it has one |
| `createdAt` | RFC 3339, whole seconds |

`kind`, `buildState` and `failureKind` are open enums (`x-openEnums`): a client reads a value a later
engine adds as unknown, and the engine refuses a stored value it does not know instead of guessing.

The files of an avatar are in `<data>/avatars/<avatarId>/` (owner-only): `source.png` (the picture, written
when the avatar is made), `package/` (the built package, written by the builder) and the build log. The
folder is removed with the avatar.

**Binding** (one per workspace, `AvatarBinding`):

| Field | Meaning |
| --- | --- |
| `workspaceId`, `avatarId` | the workspace and the avatar it has |
| `harness`, `model`, `effort` | what the avatar thinks with, so its pane can be started again the same way |
| `hearing` | `AvatarHearing {providerId, language}`: the speech-to-text provider and a BCP 47 tag |
| `voice` | `AvatarVoice`: the text-to-speech provider and voice; overrides the avatar's default |
| `persona` | plain text: how the avatar speaks and what the person asked of it (at most `avatarPersonaChars` characters; empty when none) |
| `voiceOn` | the clients that have voice on for the workspace: a paired device's `deviceId`, and `local` for the person at the computer |

A closed workspace keeps no binding in the lists. Deleting an avatar removes the bindings of closed
workspaces that name it.

**Keys.** The engine stores identifiers and settings only. A key of a speech or an image provider is never
a field of any method, never a column and never part of an event or a log line: voice providers run in
the client that has the microphone and keep their keys in that device's vault, and an image provider runs
where the avatar is built and keeps its key in the engine host's vault (ADR 0166). A request that carries
an extra field with a key in it is not rejected, and the field goes nowhere. The persona is never written
to a log either.

## 2. Methods

All are for `human_local` and `human_device`. An `agent` or a `chief` gets `forbidden_role`: the agent
allow-list of the dispatcher does not name them. Every mutation takes an `idempotencyKey`.

- `avatar.list {}` → `{avatars, bindings}`: every avatar, oldest first, and the binding of every open
  workspace that has one.
- `avatar.create {name, kind, picture, voice?, idempotencyKey}` → `{avatar}`. `picture` is an
  `AttachmentRef` of a PNG sent as attachment frames before the request (at most `avatarPictureBytes`,
  `image/png`, with the PNG signature). The avatar starts with `buildState: none`. The request's identity
  names the picture by its content, so a retry with the same key that sends the same bytes again repeats
  the call and another picture under that key is `idempotency_conflict`. A blank name, a picture that is
  not a PNG or is too large, and a library that holds `avatarCount` avatars are `invalid_params`; nothing
  is made.
- `avatar.rename {avatarId, name, idempotencyKey}` → `{avatar}`; `not_found` for an unknown avatar.
- `avatar.delete {avatarId, idempotencyKey}` → `{avatarId, removed}`. An avatar that an open workspace
  has is `pane_busy`: the person gives that workspace another avatar first. An avatar being built can be
  deleted. `not_found` for an unknown avatar.
- `avatar.assign {workspaceId, avatarId, harness, model?, effort?, hearing?, voice?, persona?, voiceOn?,
  idempotencyKey}` → `{binding}`: gives an open workspace the avatar, replacing its binding. What the
  call does not carry is cleared (`persona` becomes empty), except `voiceOn`: it is a boolean for the
  caller's own client only, absent leaves it as it was, and the entries of the other clients are never
  changed by it. `not_found` for a workspace that is closed or unknown and for an unknown avatar. In
  this slice the engine records the binding; starting the avatar's pane as the workspace's `chief` agent
  is slice 2.
- `avatar.build {avatarId, idempotencyKey}` → `{avatar}`: asks for a build. An avatar with `none` or
  `failed` becomes `building` (and forgets its failure); one already `building` is answered as it is,
  with no event; a `ready` one is `transition_refused`, because another picture is another avatar. In this
  slice nothing runs the build: it waits as `building` until the builder of slice 9 reports.
- `avatar.package.read {avatarId, file, offset, length?}` → `{avatarId, file, offset, size, chunk}` with
  the bytes as an attachment frame, so a phone can fetch a ready package in pieces. `file` is a path inside
  `package/` written with `/` and with no empty, `.` or `..` part, and no `\` or `:`; the package's own
  manifest lists the other files. The chunk is at most `length` bytes and at most `attachmentPayloadBytes`;
  `offset` is a decimal string, `size` the whole file's size, and an `offset` equal to `size` answers an
  empty chunk. An avatar that is not `ready` has no package (`not_found`), nor has a path that is not a
  regular file inside the folder, a link that leaves it included; `offset` past the end is
  `invalid_params`.

## 3. Events

- `avatar.changed {avatarId, avatar?, workspaceId?, binding?}` is durable. After `avatar.create`,
  `avatar.rename` and `avatar.build` it carries the new `avatar`. After `avatar.assign` it carries the
  `binding` and its `workspaceId`, and `avatarId` is the avatar now bound. After `avatar.delete` it names
  the `avatarId` and has neither `avatar` nor `binding`: the avatar is gone, and so are its bindings.
- `avatar.build_progress {avatarId, done, total}` is not durable (a client that was away reads the state
  from `avatar.list`). It tells how many frames of a build are made. The builder of slice 9 raises it;
  nothing raises it in this slice.

## 4. Persistence

Migration `038-avatars.sql` (schema 38) adds `avatars`, `workspace_avatars` and `workspace_avatar_voice`.
An engine that is upgraded starts with an empty library and no binding. Kinds and states are stored as
their wire names.

## 5. Outside this version

The workspace folder flow, the avatar pane and the persona delivery (slice 2); the package format, the
runtime and the animation (slices 3 to 5); speech providers and the voice loop (slices 6 to 8); the
builder, the `soyeht avatar` commands and the `avatar-builder` skill (slices 9a and 9b); the screens
(slices 10 to 12). Taking an avatar off a workspace without giving it another is not part of this
version: a workspace that has one keeps one until it is closed.
