#if os(macOS)
  import AppKit

  /// SwiftTerm draws its scroller as a standalone `NSScroller`, so no scroll view fades it and
  /// it stays on screen. This gives it the overlay behaviour of every other pane scroll view:
  /// hidden at rest, shown while the display is scrolled back from the live bottom or while the
  /// pointer is over it, then faded out. A hidden scroller still takes clicks, and showing it on
  /// hover keeps it draggable.
  @MainActor final class TerminalScrollerFade: NSObject {
    static let visibleDuration: TimeInterval = 1.2
    static let fadeDuration: TimeInterval = 0.3

    private weak var terminal: NSView?
    private weak var scroller: NSScroller?
    private var trackingArea: NSTrackingArea?
    private var pointerInside = false
    private var hideWork: DispatchWorkItem?

    init(terminal: NSView) {
      self.terminal = terminal
    }

    var isShowing: Bool { (scroller?.alphaValue ?? 0) > 0 }

    func attach() {
      guard let found = terminal?.subviews.lazy.compactMap({ $0 as? NSScroller }).first else {
        return
      }
      if found !== scroller {
        scroller = found
        found.alphaValue = 0
        let area = NSTrackingArea(
          rect: .zero, options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
          owner: self, userInfo: nil)
        found.addTrackingArea(area)
        trackingArea = area
      }
    }

    func displayScrolled(awayFromBottom: Bool) {
      if awayFromBottom || isShowing { show() }
    }

    @objc func mouseEntered(with event: NSEvent) {
      pointerInside = true
      show()
    }

    @objc func mouseExited(with event: NSEvent) {
      pointerInside = false
      scheduleHide()
    }

    private func show() {
      attach()
      guard let scroller else { return }
      hideWork?.cancel()
      if scroller.alphaValue < 1 { scroller.alphaValue = 1 }
      scheduleHide()
    }

    private func scheduleHide() {
      hideWork?.cancel()
      let work = DispatchWorkItem { [weak self] in
        MainActor.assumeIsolated { self?.hide() }
      }
      hideWork = work
      DispatchQueue.main.asyncAfter(deadline: .now() + Self.visibleDuration, execute: work)
    }

    private func hide() {
      guard !pointerInside, let scroller else { return }
      NSAnimationContext.runAnimationGroup { context in
        context.duration = Self.fadeDuration
        scroller.animator().alphaValue = 0
      }
    }
  }
#endif
