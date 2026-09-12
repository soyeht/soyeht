# ADR 0048 · Editor search scopes

Date: 2026-09-12
Status: accepted for slice 4

## Context

The editor contract assigns project text search, project replacement, fuzzy
open by file name, and search-everywhere to one `file.search` method. The
original request shape carried a query but did not identify whether that query
was a literal content search or a fuzzy path search. Inferring the operation
from case sensitivity, replacement presence, or a query prefix would create an
undocumented second protocol.

## Decision

`FileSearchParams` gains the optional closed `scope` value `content | path`.
An absent value means `content`, preserving the first schema and its vectors.
Content scope performs literal project text search and optional replacement.
Path scope performs fuzzy matching over canonical workspace-relative file
paths and rejects a replacement. The caller already knows the scope, so the
result does not duplicate a match-kind field.

A path match uses line zero and UTF-16 columns over the matching portion of the
path; its preview is the workspace-relative path. Search-everywhere combines
path results with commands, settings, and later language-server symbol results
in the Mac client. `Cmd+P` uses path scope. Project search uses content scope.

Only the engine enumerates workspace files. Both scopes exclude paths outside
the canonical workspace, ignored source-control entries, the source-control
metadata directory, and non-UTF-8 or over-limit files. Replacement revalidates
each file immediately before its conditional atomic write.

## Consequences

The two different search meanings stay explicit and generated in every client
language. Existing unreleased callers that omit `scope` retain content-search
behavior. Adding the field does not change any slice 1–3 method, event, or
field.
