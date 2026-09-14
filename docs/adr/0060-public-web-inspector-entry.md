# ADR 0060 · Public Web Inspector entry

Status: accepted on 2026-09-13.

## Context

The browser pane must expose Web Inspector without relying on private WebKit interfaces. The
public `WKWebView.isInspectable` property makes an embedded page available to Safari Web
Inspector. WebKit does not publish an API that directly presents the inspector for a particular
view.

## Decision

Every browser pane sets `isInspectable` to true. Its DevTools control explains the system path,
opens Safari, and directs the person to the Develop menu. The application does not call private
selectors or inspect internal WebKit objects.

The same control group owns zoom, bounded find-in-page text, and bounded screenshot capture.
Screenshot data may be copied to the pasteboard or written through a person-selected save panel.

## Consequences

Inspection uses the supported system workflow and remains compatible with signed builds. Opening
the inspector takes one selection in Safari after pressing the control. A future public API may
replace this handoff without changing the pane or engine contracts.
