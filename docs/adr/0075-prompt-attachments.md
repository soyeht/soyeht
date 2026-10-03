# ADR 0075: Prompt attachments

- Status: accepted
- Date: 2026-10-03

## Context

The owner wants to drop, paste or pick images, PDFs and any other file into
the agent composer on the Mac, iPhone and iPad, see them as removable chips,
and have the agent receive them with the prompt. A paired iPhone must be able
to attach a photo that exists only on the phone.

`pane.prompt` already carried `AttachmentRef` PNGs for picked web elements
(ADR 0059). The engine kept their bytes in `agent_prompt_attachments` and sent
every one as an ACP `image` block, whatever the agent advertised. That breaks
ACP v1, where a client may send `image`, `audio` and embedded `resource`
blocks only when `initialize` returned `promptCapabilities.image`, `.audio` or
`.embeddedContext`, and where only `text` and `resource_link` are baseline.
A native replay that contained an image also failed, because the engine
accepted only text in replayed user messages.

The pinned adapters (`acp_registry_snapshot.json`) accept this, read from their
published `dist` code:

| | claude-agent-acp 0.77.0 (`dist/acp-agent.js`) | codex-acp 1.11.0 (`dist/index.js`) |
|---|---|---|
| `promptCapabilities` | `image`, `embeddedContext` (initialize, line 926) | `image`, `embeddedContext` (initialize, line 32318) |
| `image` | base64 `data` becomes an Anthropic image; a `uri` is used only when it is `http` (`promptToClaude`, line 6981) | becomes a `data:` URL unless `uri` is http, https or data (`buildPromptItems`, line 29015) |
| `resource` with `text` | a link plus `<context ref=…>` text | a link plus `<context ref=…>` text |
| `resource` with `blob` | ignored | image types become an image; any other blob is pasted into the text as base64 |
| `resource_link` | text `[@name](file://…)` | text `[@name](file://…)` |
| `audio` | ignored | dropped |
| replay of a user image | `user_message_chunk` with an `image` block (line 7346) | images from history as image blocks |

Both providers take PNG, JPEG, GIF and WebP images.

## Decision

**Protocol.** `PanePromptParams.attachments`, `PromptItem.attachments` and the
new `TranscriptMessage.attachments` hold `PromptAttachment`: `kind` (`image`,
`audio`, `text`, `file`), `name`, `mimeType`, `bytes`, an optional client
`thumbnail` (PNG or JPEG, at most 6 KiB) and exactly one source on a request:

- `attachmentId`: the bytes arrive as type-3 frames before the request, the
  existing staging path of ADR 0063 and 0064. It works over the paired
  device's WebSocket, so a phone photo reaches the Mac.
- `data`: inline base64, at most 192 KiB, for small files and scripts.
- `path`: a file already on the engine's Mac. It is linked where it is and
  never copied, so a large file costs nothing.

Limits are generated from `x-limits`: 20 files, 10 MiB of sent bytes per file
and 20 MiB per prompt. A breach fails with the new error kind
`attachment_limit_exceeded` and a message that names the file and the limit.
Frames over the byte or count limit no longer close the connection: the
assembler discards them and the request that names them fails. Every
committing request leaves the assembler empty.

**Staging (engine).** `agents::attachments` checks each description (MIME type
consistent with the kind, thumbnail format and size, declared size), then
names sent bytes `<data dir>/prompt-attachments/<sha256>/<file name>` and
rewrites the request's attachments as these paths. The mutation identity then
names content: a retry with the same bytes repeats, different bytes conflict.
The bytes are written only after the queue accepted the prompt and before the
commit, through `immutable_file::publish` (temporary file, sync, exclusive hard
link, identical content accepted), now shared with the writing images of ADR
0064. A path source is canonicalized and must be a readable regular file.
Queued prompts keep paths only; schema 30 drops `agent_prompt_attachments` and
the earlier reference shape from queued payloads.

**Delivery (soyeht-acp).** `Initialization` records `promptCapabilities`.
`soyeht_acp::prompt` decides each block: `image` for PNG, JPEG, GIF and WebP
when `image` is advertised, `audio` when `audio` is, an embedded text
`resource` for a UTF-8 text file when `embeddedContext` is, and otherwise a
`resource_link` with the file URI, name, MIME type and size. Files are inlined
in order while the encoded `session/prompt` stays within one 1 MiB ACP line;
the rest are linked. The engine reads only files that could be inlined. The
task also allowed an absolute path in the text as a last fallback. It is not
needed: `resource_link` is mandatory for every ACP v1 agent, and both adapters
render it as text that holds the absolute path.

**Transcript.** The first transcript message of a prompt carries its
attachments: kind, name, size, path and thumbnail, never bytes, so a reloaded
transcript shows the chips. A native replay turns replayed `image`, `audio`,
`resource` and `resource_link` blocks into attachments of the replayed user
message instead of failing the load.

**Clients (SoyehtClient).** Shared by Mac, iPhone and iPad:
`AgentAttachmentDraft` and `AgentAttachmentList` (model and limits),
`AgentAttachmentStaging` (kind and MIME type from `UTType`; images re-encoded
as PNG or JPEG when the type is not inline-ready, such as TIFF or HEIC, or
when larger than 2048 pixels or 700 KB; 160-pixel thumbnails),
`AgentPasteboardContent` (copied files attach, Finder's file name text
notwithstanding; an image attaches unless the copy is text),
`AgentComposerAttachments` (the composer's list and staging off the main
actor) and `AgentAttachmentPresentation` (chip text and icon). On the Mac,
files other than images are linked by path; a paired device sends bytes.
`EngineClient.promptAttachments` merges picked web elements with the
composer's files, so there is one send path.

**UI.** Thin views over that state in the existing composer: chips above the
text field, a "+" that opens the file panel on the Mac and a menu with the
photo library, Files and Paste on iPhone and iPad, and a drop target over the
whole conversation. On the Mac, Command-V in the focused composer attaches
copied files and images; text still pastes as text. The plain text field
cannot take an image, so a key monitor scoped to the composer's window reads
Command-V first. Colors come from the conversation style's palette; the chips
are flat.

## Consequences

- An agent receives only blocks it advertised; an agent without capabilities
  still gets every file as a link.
- Sent bytes live as immutable files in the data directory and stay after
  delivery, because links in the agent's history point at them. Removing
  them with their pane or workspace is a follow-up.
- Edit > Paste from the menu bar and a hardware Command-V on iPad paste text
  only; the Mac shortcut and the iPhone and iPad Paste menu item attach.
- Claude ignores binary resources, so a PDF reaches it as a link that its
  file tools read, not as a document block.
- A prompt still needs text, as before.
