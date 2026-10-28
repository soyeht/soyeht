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
    settle(host) { overlaid(descendants(of: host)) }

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
    settle(host) { overlaid(scrollViews) }
    for scrollView in scrollViews {
      #expect(scrollView.scrollerStyle == .overlay)
      #expect(scrollView.autohidesScrollers)
    }
  }

  /// Lays the view out and runs the main loop until `done` holds: at least 20 turns and up to 30
  /// seconds. While this test turns the loop, the main queue also runs other tests' main-actor
  /// work, so on a busy machine a turn can take seconds and a short wall-clock limit ends after
  /// a few turns. The expectations that follow say what is still wrong after the wait.
  @MainActor private func settle(_ host: NSView, until done: () -> Bool) {
    let deadline = Date().addingTimeInterval(30)
    var turns = 0
    repeat {
      host.layoutSubtreeIfNeeded()
      RunLoop.current.run(until: Date().addingTimeInterval(0.05))
      turns += 1
    } while !done() && (turns < 20 || Date() < deadline)
  }

  @MainActor private func overlaid(_ scrollViews: [NSScrollView]) -> Bool {
    !scrollViews.isEmpty
      && scrollViews.allSatisfy { $0.scrollerStyle == .overlay && $0.autohidesScrollers }
  }

  @MainActor private func descendants(of view: NSView) -> [NSScrollView] {
    view.subviews.flatMap { child in
      ((child as? NSScrollView).map { [$0] } ?? []) + descendants(of: child)
    }
  }
#endif
