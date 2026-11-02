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

    let first = await MountedTranscript.mount(client: client) { $0.isAtBottom }
    #expect(first.isAtBottom, "a pane never seen opens at the bottom")
    await first.scroll(toFraction: 0.4)
    #expect(!first.isAtBottom)
    let leftAt = try #require(first.bottomRow(client))
    #expect(leftAt < 39, "scrolled up to row \(leftAt)")
    await first.unmount()
    // A view that is gone is not restored: the next one must not record what it reports first.
    #expect(first.isRestoring, "an unmounted view leaves the scroll waiting for a restoration")

    let again = await MountedTranscript.mount(client: client) { $0.offset > 0 }
    #expect(!again.isAtBottom, "scrolled up: not pushed to the bottom")
    #expect(again.offset > 0, "not at the first prompt")
    // One wheel tick makes the scroll view report the row now at the bottom edge.
    await again.scroll(by: 2)
    let returnedTo = try #require(again.bottomRow(client))
    #expect(abs(returnedTo - leftAt) <= 3, "left at row \(leftAt), returned to \(returnedTo)")
    await again.scroll(toFraction: 1)
    #expect(again.isAtBottom)
    await again.unmount()

    let following = await MountedTranscript.mount(client: client) { $0.isAtBottom }
    #expect(following.isAtBottom, "following the agent: back at the bottom")
    await following.unmount()
  }

  @MainActor private final class MountedTranscript {
    let host: NSHostingView<AnyView>
    let window: NSWindow
    private let client: EngineClient

    /// Mounts the view and waits until it has been laid out, its restoration has ended and
    /// `placed` holds: the place the test asserts next. A scroll made before the restoration
    /// ends is swallowed and then undone by it, so this never returns earlier.
    static func mount(
      client: EngineClient, placed: @MainActor (MountedTranscript) -> Bool = { _ in true }
    ) async -> MountedTranscript {
      let mounted = MountedTranscript(client: client)
      let scroll = client.agentPanes[PaneInteriorFixture.paneId]?.scroll
      await mounted.settle { scroll?.isRestoring != true && placed(mounted) }
      return mounted
    }

    private init(client: EngineClient) {
      self.client = client
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

    /// What a settle watches: it is finished when none of these moved for 4 turns in a row.
    private struct Movement: Equatable {
      var height: CGFloat
      var offset: CGFloat
      /// The viewport reports a place on a turn of SwiftUI's own, some turns after the scroll
      /// that caused it, and a scroll in steps reports a row for each step.
      var place: AgentTranscriptScroll.Place?
    }

    /// Past this, a wait that has not ended is a hang, not a slow machine.
    private static let hangGuard: TimeInterval = 600

    /// Lays the view out until the transcript has stopped moving (its height, offset and
    /// reported place the same for 4 turns in a row) and `done` holds: at least 12 turns. SwiftUI
    /// builds the rows, scrolls to the place it restores and reports where the viewport is on its
    /// own turns of the main loop, which are shared with the other tests' main-actor work: with
    /// the whole suite running, the 3 sleeps of one restoration took 12 seconds instead of 0.3.
    /// A wait on what those turns produce therefore ends when it has happened, not at a time;
    /// `hangGuard` only turns a wait that can never end into a failure.
    func settle(until done: @MainActor () -> Bool = { true }) async {
      let giveUp = Date().addingTimeInterval(Self.hangGuard)
      var turns = 0
      var steady = 0
      var last: Movement?
      repeat {
        host.layoutSubtreeIfNeeded()
        try? await Task.sleep(for: .milliseconds(50))
        let now = Movement(
          height: transcript?.documentView?.frame.height ?? 0, offset: offset,
          place: client.agentPanes[PaneInteriorFixture.paneId]?.scroll.place)
        steady = now == last ? steady + 1 : 0
        last = now
        turns += 1
      } while (turns < 12 || steady < 4 || !done()) && Date() < giveUp
      if !done() { Issue.record("the transcript never settled: \(String(describing: last))") }
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

    var isRestoring: Bool {
      client.agentPanes[PaneInteriorFixture.paneId]?.scroll.isRestoring != false
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
      // The scroll view reports where it stopped on a turn of its own; wait for the pane to
      // hold that place, as the view records it, before the test reads it.
      await settle {
        let place = self.client.agentPanes[PaneInteriorFixture.paneId]?.scroll.place
        if fraction == 1 { return place == .bottom }
        if case .row = place { return true }
        return false
      }
    }
  }
#endif
