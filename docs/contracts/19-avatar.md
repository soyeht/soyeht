# Contract 19 · The workspace's avatar

Status: v1, slices 1 and 2 of ADR 0166 (the library, the binding, the project folder and the avatar pane;
the build, the package, the animation and the voice loop are later slices). Code: `soyeht_engine::avatars`,
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
  workspace that is closed or unknown and for an unknown avatar. `harness` must be one of the profile's
  (`harness_unavailable` otherwise), whether or not the workspace has a pane. `acknowledgedPackage` is the
  one of `pane.create` (ADR 0143), for the pane this call starts or switches. The call also **brings the
  avatar's pane to the binding** (section 6) and answers `{binding, paneId}`:
  - a workspace with no avatar pane open that is alive gets one **started**, in the same change as the
    binding. A request that `pane.create` would refuse (`adapter_not_acknowledged`, a harness that
    cannot be resolved) leaves no binding. A pane that is created and then fails to start leaves the
    binding and the failed pane, as `pane.create` leaves a pane that failed to start;
  - a workspace whose avatar pane is open and alive gets no other. The binding is replaced and `paneId`
    names that pane, which is changed in the same call, **before** the binding, so a refusal leaves the
    binding as it was:
    - a `harness` that is not the pane's is `pane.harness.switch` (with the call's `model` and `effort`,
      and the errors of that method, `pane_busy` among them; the switch finishes after the reply, as it
      does for that method). **Until the switch ends the binding keeps the harness, model and effort the
      pane has**, and the reply says so; the end of the switch records the harness, model and effort it
      asked for, one revision on, with `avatar.changed` (nothing is recorded again when the binding
      already says so), and a switch that fails records nothing, so the binding never names a harness the
      pane is not on. A pane that `pane.harness.switch` refuses for its phase (it asks for a sign-in, is
      coming back, or failed) is **replaced**: it is closed and another is started with the harness
      asked for, in the same call;
    - a `model` or `effort` that is not the session's is put in the pane's requested options, both in
      one change, and the engine applies them one at a time when the pane is idle (`pane.config_changed`,
      or `pane.config_failed` for one the session does not offer);
    - a changed persona or avatar is owed to the pane's next prompt, whatever its adapter has (section
      6);
  - an avatar pane that **failed for good** (its agent is faulted, or its pane is dead) is not the
    workspace's: the call closes it and starts another, so a workspace has one avatar pane;
  - the pane is named for the avatar, and is named again when the avatar is renamed or another avatar
    is given;
  - a `pane.harness.switch` of the avatar pane that a person asks for directly is the avatar thinking
    with another harness: the same rule records the harness, model and effort it asked for when it ends.
- `project.folder.create {name, idempotencyKey}` -> `{path}`: makes an empty folder for a project that
  is not an app or a repository (a book being read, a school subject) under `<data>/projects/`, owner-only,
  named from `name` (composed to NFC, lower-case letters and digits joined by hyphens, at most
  `projectFolderNameChars` characters; `-2`, `-3` when the name is taken, and a name Windows keeps for a
  device, `COM1` to `COM9` and `LPT1` to `LPT9` with the superscript digits included, and `CON`, `PRN`,
  `AUX`, `NUL`, gets a `project-` prefix). `path` is the
  canonical absolute path, which the client gives to `workspace.create`. `name` is 1 to
  `projectNameChars` characters after trimming; another is `invalid_params`. The engine writes nothing in
  the folder and removes nothing from it, closing the workspace included. Only a person calls it (an
  `agent` or a `chief` gets `forbidden_role`).

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

The package format, the runtime and the animation (slices 3 to 5); speech providers and the voice loop
(slices 6 to 8); the builder, its calls, the `soyeht avatar` commands and the `avatar-builder` skill
(slices 9a and 9b); the preset pack, which is what produces `kind: preset`; the screens (slices 10 to 12).

## 6. The avatar pane (slice 2)

`avatar.assign` starts the pane as an agent pane of the binding's harness, model and effort, named for
the avatar, in the workspace's folder, and **as the workspace's `chief`** (contract 08 section 2) from
its first session. The lead prompt that `pane.role.set` queues for a chief is not queued: what the
avatar is for is its persona. The pane's agent state carries that it is the avatar's, so a restart, a
recovery and a harness switch keep it.

**The persona** is plain text: `You are <name>, the avatar agent of this project.`, then the persona of
the binding. Soyeht adds no other text and no rule; what the avatar does in a project is its own job
from plain instructions. It reaches the harness by the channel that harness honours, measured per
harness in `docs/research/avatar-persona-delivery.md`:

1. an adapter that advertises the ACP `systemPrompt` session capability (an unstable field of the
   protocol) gets it as the session's system prompt, appended, in `session/new` and in `session/load`
   (it is sent only to such an adapter: the two measured that do not advertise it ignore the field
   without an error). The open session keeps the persona it was opened with: a persona the person
   changed reaches the next prompt as in 2, and the system prompt catches up the next time such a
   session opens (a restart of the process or of the engine, or a harness switch);
2. any other is owed the persona by its **next prompt**. The pane is marked when a session of it is
   created (an adapter with a system prompt is not marked by that) and when the persona or the avatar of
   the open pane changed (on every adapter). The next `session/prompt` the engine builds has the persona
   as a text block ahead of its own, after a line saying these are standing instructions from the
   application, and before a last line, `(End of the standing instructions.)`, that ends the block; a
   persona that holds that line is delivered with the full stop of it removed, so the line that ends
   the block is the engine's alone. The pane keeps owing the persona **until the harness has answered
   the prompt that carried it** (a turn that was cancelled is an answer): a prompt that fails, or a
   process that is lost with it, leaves the pane owing it and the next prompt carries it again. It is no
   prompt of the queue: it cannot be removed, paused or overtaken by the person's prompts, and it costs
   no turn. It is not shown as a message: a replay of the session (`session/load`) holds the text of a
   user message while it could still be the block, whole, in chunks, or joined to the text after it, and
   cuts it out between the first line and the end line (a message that only began like the first line
   is shown whole). A `session/load` does not send the block again: the conversation it loads has it.

The text is read from the binding each time it is delivered (and when a harness switch opens its
candidate session), so the persona the person saved last is the one the next delivery carries. At the
limits (`avatarPersonaChars`, `avatarNameChars`) the block is within `promptBytes`. No file is written
in the project folder: a workspace can be a folder the person already has. The persona is never part of
a log line.
