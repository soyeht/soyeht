# Writing surface rendering spike

Date: 2026-09-14

## Question

The writing surface needs hybrid Markdown rendering, an active-line raw-syntax
view, agent suggestion overlays, pasted images, and bounded performance on a
large document. The spike compared the existing native TextKit direction with
an embedded WebKit editing surface before any writing feature landed.

## Method

A release-compiled Swift probe ran both candidates in one process on the Dev
Mac. It generated a 100,000-line Markdown document containing 11,338,058 UTF-8
bytes. Both candidates loaded the same 2,000-line viewport, matching the owned
file protocol rather than placing the entire document in a client view.

Each candidate performed the same checks:

- lay out the bounded viewport;
- switch the active line while showing raw syntax only there;
- apply and move an agent suggestion highlight over 20 successive lines;
- insert a 96 by 64 image at the active line.

The native candidate used `NSTextStorage`, `NSLayoutManager`, `NSTextView`, and
`NSTextAttachment`. The web candidate used a non-persistent `WKWebView`, a
bounded `contenteditable` document, CSS for the hybrid line state, a marked
suggestion, and an inline image. The probe source and binary remained in
`/private/tmp/soyeht-slice6-prose-spike`; they are not product code.

## Results

| Check | Native TextKit | Embedded WebKit |
| --- | ---: | ---: |
| Initial visible layout | 69.498 ms | 147.106 ms |
| Active-line and suggestion update, p95 | 1.769 ms | 99.729 ms |
| Image insertion | 1.694 ms | 0.860 ms |
| Hybrid preview expressed | yes | yes |
| Suggestion overlay expressed | yes | yes |
| Image attachment expressed | yes | yes |

The native path stayed within one AppKit text system and reused the existing
viewport and edit-coordinate model. The web path could express the features,
but every active-line change crossed the process boundary through JavaScript.
It also introduced a second document selection, paste, undo, and accessibility
model beside the existing native editor.

## Decision input

Use native TextKit for the writing surface. It met all four capability checks,
had the lower initial-layout time, and kept the frequent active-line update
well below one display frame. Images were fast in both candidates.

The product implementation remains viewport-bound. This spike does not claim
that either candidate can safely load an unbounded document. The existing
50 MiB owned-file limit and the protocol viewport limit remain hard bounds.

