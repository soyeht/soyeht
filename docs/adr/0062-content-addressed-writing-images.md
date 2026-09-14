# ADR 0062: Content-addressed writing images

Date: 2026-09-14
Status: Accepted

## Context

Presentation documents accept pasted and dropped PNG images. The engine owns file I/O, while the Mac client owns the presentation surface. A binary transfer can be interrupted after the image reaches the engine or after the filesystem changes but before the mutation receipt is durable.

## Decision

`writing.format` accepts one optional `AttachmentRef` when its format kind is `image`. The connection assembles at most `WRITING_IMAGE_BYTES`, verifies the declared PNG metadata and exact byte count, and adds the SHA-256 digest to the mutation identity before checking an existing receipt.

The engine derives `assets/<sha256>.png` beside the Markdown document. It prepares the complete bytes in the destination directory, synchronizes them, and publishes the content with an exclusive hard link. An existing target is accepted only when it is a regular non-symlink file with byte-for-byte identical content. The engine never overwrites an asset. The Markdown edit refers to the relative asset path and commits through the existing owned-file mutation.

The asset write precedes the database mutation. A crash before publication can leave an unreferenced temporary file; a crash after publication is recovered by accepting the identical content and completing the same idempotent Markdown mutation. Temporary files are never referenced by a document. The normal operation removes its temporary file on every success or error path.

The existing path-valued image format remains valid for already-local assets. Binary attachments are additive and do not change protocol version 1.

## Consequences

The client cannot select an arbitrary workspace path for pasted bytes. Content identity determines the filename, retries cannot duplicate or clobber data, and the engine remains the only file writer. Filesystem publication and the SQLite receipt cannot share one transaction, so recovery relies on immutable content-addressed bytes and the existing idempotency receipt.
