# Contract 19 · The workspace's avatar

Status: v1, slice 1 of ADR 0166 (the library and the binding; the avatar pane, the build, the package,
the animation and the voice loop are later slices). Code: `soyeht_engine::avatars`,
`soyeht_store::avatars`. Limits: `avatar*` of `x-limits` in `contracts/protocol.json`.

A workspace is a project, and it has one avatar agent: a person-shaped face the person talks to by voice
or by text, thinking with the harness the person chose. This contract is what the engine stores and
serves for that: a library of avatars that every workspace can reuse, and the binding of one avatar to a
workspace with the settings that avatar needs. It adds methods, an event and a capability and changes
no existing method, type or event (ADR 0161). The pictures, the speech and the animation run in the
clients and the builder; the wire carries identifiers, settings and the bytes of a picture, never audio.

An engine offers the capability `avatars` in the `capabilities` of `welcome` and of the snapshot. A client
shows the avatar screens only when it is offered.

## 1. What is stored

**Avatar** (the library is global to the person):

| Field | Meaning |
| --- | --- |
| `avatarId`, `name` | the identifier the engine made, and the person's name for it (1 to `avatarNameChars` characters, trimmed). **Names are not unique**: two avatars may have the same one, and a client tells them apart by the picture and the date |
| `kind` | `uploaded` or `generated`: where the picture came from. An open string (at most `avatarKindChars`): a client that reads another value shows the avatar as it does an uploaded one. `preset` is reserved for the preset pack: nothing produces it yet, and `avatar.create` refuses it until something does |
| `sourceHash` | the lowercase hexadecimal SHA-256 of the source picture |
| `imageProvider`, `imageModel` | the catalogue ids of the image provider and the image model that made the picture or built the avatar, when the person said (ADR 0166 section 2). Open strings (at most `avatarCatalogueIdChars`) that the engine keeps and does not interpret: the client's catalogue names them |
| `voice` | the default `AvatarVoice {providerId, voiceId}` of the avatar, when it has one |
| `createdAt` | RFC 3339, whole seconds |

The files of an avatar are in `<data>/avatars/<avatarId>/` (owner-only): `source.png`, the picture, written
when the avatar is made. The folder is removed, off the Core lock, when the avatar is deleted, and a folder
the library does not know (a creation that did not commit, a removal that did not finish) is removed when
the engine starts.

**Provisional (ADR 0166 section 6).** The build state of an avatar, its package and the files of the
package folder are not part of this contract. The package format stays unstable (0.x) until the renderers
and the builder have all consumed it, so nothing here says what a package holds, what its hash covers or
how its files are listed, and an avatar is not final once built: it can be built again. The slice that
builds avatars adds those fields, its calls and its events together with what produces them.

**Binding** (one per open workspace, `AvatarBinding`):

| Field | Meaning |
| --- | --- |
| `workspaceId`, `avatarId` | the workspace and the avatar it has |
| `harness`, `model`, `effort` | what the avatar thinks with, so its pane can be started again the same way |
| `hearing` | `AvatarHearing {providerId, language}`: the speech-to-text provider and a BCP 47 tag |
| `voice` | `AvatarVoice`: the text-to-speech provider and voice; overrides the avatar's default |
| `persona` | plain text: how the avatar speaks and what the person asked of it (at most `avatarPersonaChars` characters; empty when none) |
| `revision` | a decimal that starts at `1` and grows by one with every `avatar.assign` that changes the binding |

A binding goes with its workspace when the workspace is closed (a closed workspace is never opened again),
persona included. Whether a client has voice on is that client's own preference, since it is the client
that holds the microphone: the engine does not keep it and the wire does not carry it.

**Keys.** The engine stores identifiers and settings only. A key of a speech or an image provider is never
a field of any method, never a column and never part of an event or a log line: voice providers run in
the client that has the microphone and keep their keys in that device's vault, and an image provider runs
where the avatar is built and keeps its key in the engine host's vault (ADR 0166). A request that carries
an extra field with a key in it is not rejected, and the field goes nowhere. The persona is never written
to a log either.

## 2. Methods

All are for `human_local` and `human_device`. An `agent` or a `chief` gets `forbidden_role`: the agent
allow-list of the dispatcher does not name them. Every mutation takes an `idempotencyKey`. The library is
not in the snapshot: a client reads it with `avatar.list` when it connects and again when it reconnects,
and follows `avatar.changed` meanwhile (an event it missed while away is replayed by the event cursor like
any other durable one).

- `avatar.list {}` → `{avatars, bindings}`: every avatar, oldest first, and the binding of every open
  workspace that has one.
- `avatar.create {name, kind, picture, imageProvider?, imageModel?, voice?, idempotencyKey}` → `{avatar}`.
  `picture` is an `AttachmentRef` of a PNG sent as attachment frames before the request: `image/png`, at
  most `avatarPictureBytes`, a well-formed PNG header (the signature, then the `IHDR` chunk) that declares
  neither a width nor a height of zero or over `avatarPictureDimension`. The engine reads the header and
  nothing more: it decodes no image. A blank name, a `kind` that is not `uploaded` or `generated`, a picture
  that is not such a PNG or was never sent (or is not the size the request says) and a library that holds
  `avatarCount` avatars are `invalid_params`; a picture over `avatarPictureBytes`, or a transfer over what
  the connection takes, is `attachment_limit_exceeded`. Nothing is made, and the frames of the refused request are dropped
  (`method_family::ATTACHMENTS`, which the contract derives, names the methods whose frames are dropped when they are refused; a request that takes no attachment and finds frames pending is refused and leaves them for the request they belong to). The request's identity names the picture by its content, so a retry with the same key that sends
  the same bytes again repeats the call, and another picture under that key is `idempotency_conflict`.
- `avatar.rename {avatarId, name, idempotencyKey}` → `{avatar}`; `not_found` for an unknown avatar.
- `avatar.delete {avatarId, idempotencyKey}` → `{avatarId, removed}`. An avatar that an open workspace has
  is `pane_busy`, refused by the store: the person gives that workspace another avatar first. `not_found`
  for an unknown avatar.
- `avatar.assign {workspaceId, avatarId, harness, model?, effort?, hearing?, voice?, persona?, baseRevision,
  idempotencyKey}` → `{binding}`: gives an open workspace the avatar, **replacing** its binding. What the
  call does not carry is cleared (`persona` becomes empty). `baseRevision` is the `revision` of the binding
  the caller read, or `"0"` when the workspace had none: a different one is `stale_version`, nothing changes,
  and the caller reads again (from `avatar.list` or `avatar.changed`) and decides. So a client holding an old
  copy, a phone that was offline, cannot overwrite what the computer wrote since. `not_found` for a
  workspace that is closed or unknown and for an unknown avatar. In this slice the engine records the
  binding; starting the avatar's pane as the workspace's `chief` agent is slice 2.

Not in this version, and why: building an avatar, its progress and reading its package come with the slice
that has something to produce a build and a package (slice 9), each with its owner and its expiry. Taking
an avatar off a workspace without giving it another is not part of this version either: a workspace that
has one keeps one until it is closed.

## 3. Events

`avatar.changed {avatarId, avatar?, workspaceId?, binding?, removed?}` is durable. After `avatar.create` and
`avatar.rename` it carries the new `avatar`. After `avatar.assign` it carries the `binding` and its
`workspaceId`, and `avatarId` is the avatar now bound. After `avatar.delete` it names the `avatarId` and has
neither `avatar` nor `binding` and carries `removed: true`: the avatar is gone. A client that has a binding replaces it by the one of
the event when the `revision` is higher.

## 4. Persistence

Migration `038-avatars.sql` (schema 38) adds `avatars` and `workspace_avatars`. An engine that is upgraded
starts with an empty library and no binding. Closing a workspace deletes its binding in the same change.

## 5. Outside this version

The workspace folder flow, the avatar pane and the persona delivery (slice 2); the package format, the
runtime and the animation (slices 3 to 5); speech providers and the voice loop (slices 6 to 8); the
builder, its calls, the `soyeht avatar` commands and the `avatar-builder` skill (slices 9a and 9b); the
preset pack, which is what produces `kind: preset`; the screens (slices 10 to 12).
