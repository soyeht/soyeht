import Foundation
import Testing

@testable import SoyehtClient

@Test func transcriptScrollStartsAtTheBottomAndFollowsTheBottomMarker() {
  let scroll = AgentTranscriptScroll()
  #expect(scroll.restoration(rowIds: ["1", "2"]) == .bottom)
  scroll.record(bottomVisibleId: "1")
  #expect(scroll.place == .row("1"))
  // The scroll view has not reported yet: keep what was recorded.
  scroll.record(bottomVisibleId: nil)
  #expect(scroll.place == .row("1"))
  scroll.record(bottomVisibleId: AgentConversationFollow.bottomAnchor)
  #expect(scroll.place == .bottom)
}

@Test func transcriptScrollReturnsToTheRowThePersonLeftOrTheBottom() {
  let scroll = AgentTranscriptScroll()
  scroll.record(bottomVisibleId: "7")
  #expect(scroll.restoration(rowIds: ["6", "7", "8"]) == .row("7"))
  // The row is not in the transcript any more: the newest message is the safe place.
  #expect(scroll.restoration(rowIds: ["8", "9"]) == .bottom)
  #expect(scroll.restoration(rowIds: []) == .bottom)
}

#if os(macOS)
  import AppKit
  import SwiftUI

  /// A workspace switch unmounts the pane's view and mounts a new one: the new view must open
  /// where the old one was left, not at the first prompt.
  @MainActor @Test func agentTranscriptKeepsItsPlaceWhenTheViewIsMountedAgain() async throws {
    let rows = (1...40).map { index in
      PaneInteriorFixture.message(
        String(format: "%03d", index), role: index.isMultiple(of: 2) ? .assistant : .user,
        text: "Row \(index). " + String(repeating: "A line of the transcript. ", count: 8))
    }
    let client = PaneInteriorFixture.client(messages: rows)
    // A pending request scrolls to the bottom by itself, which would hide the bug.
    client.agentPanes[PaneInteriorFixture.paneId]?.permission = nil

    let first = await MountedTranscript.mount(client: client)
    #expect(first.isAtBottom, "a pane never seen opens at the bottom")
    await first.scroll(toFraction: 0.4)
    #expect(!first.isAtBottom)
    let leftAt = try #require(first.bottomRow(client))
    #expect(leftAt < 39, "scrolled up to row \(leftAt)")
    await first.unmount()

    let again = await MountedTranscript.mount(client: client)
    #expect(!again.isAtBottom, "scrolled up: not pushed to the bottom")
    #expect(again.offset > 0, "not at the first prompt")
    // One wheel tick makes the scroll view report the row now at the bottom edge.
    await again.scroll(by: 2)
    let returnedTo = try #require(again.bottomRow(client))
    #expect(abs(returnedTo - leftAt) <= 3, "left at row \(leftAt), returned to \(returnedTo)")
    await again.scroll(toFraction: 1)
    #expect(again.isAtBottom)
    await again.unmount()

    let following = await MountedTranscript.mount(client: client)
    #expect(following.isAtBottom, "following the agent: back at the bottom")
    await following.unmount()
  }

  @MainActor private final class MountedTranscript {
    let host: NSHostingView<AnyView>
    let window: NSWindow

    static func mount(client: EngineClient) async -> MountedTranscript {
      let mounted = MountedTranscript(client: client)
      await mounted.settle()
      return mounted
    }

    private init(client: EngineClient) {
      host = NSHostingView(
        rootView: AnyView(
          AgentConversationView(
            client: client, paneId: PaneInteriorFixture.paneId, showsConfiguration: false,
            style: .pane(SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
          ).frame(width: 520, height: 520)))
      host.frame = CGRect(x: 0, y: 0, width: 520, height: 520)
      window = NSWindow(
        contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
      window.contentView = host
      window.alphaValue = 0
      window.ignoresMouseEvents = true
      window.orderFrontRegardless()

    }

    func settle() async {
      for _ in 0..<12 {
        host.layoutSubtreeIfNeeded()
        try? await Task.sleep(for: .milliseconds(50))
      }
    }

    func unmount() async {
      window.contentView = NSView()
      await settle()
    }

    private var transcript: NSScrollView? {
      var found: [NSScrollView] = []
      func walk(_ view: NSView) {
        if let scrollView = view as? NSScrollView { found.append(scrollView) }
        view.subviews.forEach(walk)
      }
      walk(host)
      return found.max {
        ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0)
      }
    }

    var offset: CGFloat { transcript?.contentView.bounds.origin.y ?? 0 }

    var isAtBottom: Bool {
      guard let scrollView = transcript, let document = scrollView.documentView else {
        return false
      }
      return scrollView.contentView.bounds.maxY >= document.frame.height - 24
    }

    func bottomRow(_ client: EngineClient) -> Int? {
      guard case .row(let id) = client.agentPanes[PaneInteriorFixture.paneId]?.scroll.place else {
        return nil
      }
      return Int(id)
    }

    func scroll(by pixels: Int32) async {
      guard
        let cg = CGEvent(
          scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: pixels, wheel2: 0,
          wheel3: 0),
        let event = NSEvent(cgEvent: cg)
      else { return }
      transcript?.scrollWheel(with: event)
      await settle()
    }

    /// Scrolls like the wheel does, in steps, until the offset reaches `fraction` of the travel.
    func scroll(toFraction fraction: CGFloat) async {
      guard let scrollView = transcript, let document = scrollView.documentView else { return }
      for _ in 0..<200 {
        // Rows are laid out as they scroll in, so the travel changes while it is walked.
        let travel = max(0, document.frame.height - scrollView.contentView.bounds.height)
        let remaining = travel * fraction - offset
        if abs(remaining) < 8 || (fraction == 1 && isAtBottom) { break }
        let step = Int32(max(-600, min(600, remaining)))
        guard
          let cg = CGEvent(
            scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 1, wheel1: -step, wheel2: 0,
            wheel3: 0),
          let event = NSEvent(cgEvent: cg)
        else { return }
        scrollView.scrollWheel(with: event)
        try? await Task.sleep(for: .milliseconds(20))
      }
      await settle()
    }
  }
#endif
