#if os(macOS)
  import AppKit
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The conversation's scroll views use thin overlay scrollers that hide at rest (HIG, Scroll
  /// views), also on a Mac whose mouse makes AppKit fall back to the legacy scroller, and again
  /// after the system scroller preference changes.
  @MainActor @Test func agentConversationScrollViewsUseOverlayScrollers() throws {
    let host = NSHostingView(
      rootView: AgentConversationView(
        client: PaneInteriorFixture.client(), paneId: PaneInteriorFixture.paneId,
        showsConfiguration: false,
        style: .pane(SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
      )
      .frame(width: 520, height: 520))
    host.frame = CGRect(x: 0, y: 0, width: 520, height: 520)
    let window = NSWindow(
      contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
    window.contentView = host
    settle(host)

    let scrollViews = descendants(of: host)
    #expect(!scrollViews.isEmpty)
    for scrollView in scrollViews {
      #expect(scrollView.scrollerStyle == .overlay)
      #expect(scrollView.autohidesScrollers)
    }

    // What AppKit does when a mouse is attached: the legacy scroller with a track.
    for scrollView in scrollViews {
      scrollView.scrollerStyle = .legacy
      scrollView.autohidesScrollers = false
    }
    NotificationCenter.default.post(
      name: NSScroller.preferredScrollerStyleDidChangeNotification, object: nil)
    settle(host)
    for scrollView in scrollViews {
      #expect(scrollView.scrollerStyle == .overlay)
      #expect(scrollView.autohidesScrollers)
    }
  }

  @MainActor private func settle(_ host: NSView) {
    for _ in 0..<6 {
      host.layoutSubtreeIfNeeded()
      RunLoop.current.run(until: Date().addingTimeInterval(0.05))
    }
  }

  @MainActor private func descendants(of view: NSView) -> [NSScrollView] {
    view.subviews.flatMap { child in
      ((child as? NSScrollView).map { [$0] } ?? []) + descendants(of: child)
    }
  }
#endif
