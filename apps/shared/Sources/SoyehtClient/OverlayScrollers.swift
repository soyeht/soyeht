import SwiftUI

extension View {
  /// Gives every AppKit scroll view drawn inside this view's frame thin overlay scrollers that
  /// appear while scrolling and fade out. It looks again when the frame or the window changes
  /// and when the system scroller preference changes, never on a content update, so typing and
  /// streaming do no extra work. iOS scroll indicators already behave this way.
  public func overlayScrollersWithin() -> some View {
    #if os(macOS)
      background { OverlayScrollerScope() }
    #else
      self
    #endif
  }
}

#if os(macOS)
  import AppKit

  /// Apple's scroll views have no appearance of their own: a thin overlay indicator appears
  /// while people scroll and fades out (HIG, Scroll views). AppKit falls back to the legacy,
  /// always-visible scroller with a track when a mouse is attached, so panes opt every scroll
  /// view inside them into the overlay style.
  public enum OverlayScrollers {
    @MainActor public static func apply(to scrollView: NSScrollView) {
      if scrollView.scrollerStyle != .overlay { scrollView.scrollerStyle = .overlay }
      if !scrollView.autohidesScrollers { scrollView.autohidesScrollers = true }
    }
  }

  private struct OverlayScrollerScope: NSViewRepresentable {
    func makeNSView(context: Context) -> OverlayScrollerScopeView { OverlayScrollerScopeView() }
    func updateNSView(_ view: OverlayScrollerScopeView, context: Context) {
      view.contentChanged()
    }
  }

  final class OverlayScrollerScopeView: NSView {
    /// A content update looks again at most this often, so a pane that redraws on every
    /// keystroke or streamed token does not walk the view tree each time.
    static let updateInterval: TimeInterval = 1
    /// A scroll view that is not built yet gets this many more looks, one run-loop turn apart.
    static let retries = 10

    private var observesPreference = false
    private var pending = false
    private var lastLook = Date.distantPast

    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func viewDidMoveToWindow() {
      super.viewDidMoveToWindow()
      if !observesPreference {
        observesPreference = true
        // AppKit drops the overlay style when the system preference changes; apply it again.
        NotificationCenter.default.addObserver(
          self, selector: #selector(preferredStyleChanged),
          name: NSScroller.preferredScrollerStyleDidChangeNotification, object: nil)
      }
      configureSoon()
    }

    @objc private func preferredStyleChanged() { configureSoon() }

    override func setFrameSize(_ newSize: NSSize) {
      super.setFrameSize(newSize)
      configureSoon()
    }

    func contentChanged() {
      if Date().timeIntervalSince(lastLook) >= Self.updateInterval { configureSoon() }
    }

    /// AppKit builds a SwiftUI scroll view's backing view after this marker, so the look waits
    /// a run-loop turn and tries again while the frame holds no scroll view yet.
    func configureSoon(retries: Int = OverlayScrollerScopeView.retries) {
      guard !pending else { return }
      pending = true
      RunLoop.main.perform(inModes: [.common]) { [weak self] in
        MainActor.assumeIsolated {
          guard let self else { return }
          self.pending = false
          self.lastLook = Date()
          if self.configure().isEmpty, retries > 0 { self.configureSoon(retries: retries - 1) }
        }
      }
    }

    /// The scroll views whose centre lies inside this marker's frame, found from the window's
    /// content view down.
    @discardableResult func configure() -> [NSScrollView] {
      guard let window, let root = window.contentView, !bounds.isEmpty else { return [] }
      let scope = convert(bounds, to: nil)
      var found: [NSScrollView] = []
      var stack: [NSView] = [root]
      while let view = stack.popLast() {
        if let scrollView = view as? NSScrollView {
          let frame = scrollView.convert(scrollView.bounds, to: nil)
          if scope.contains(CGPoint(x: frame.midX, y: frame.midY)) {
            OverlayScrollers.apply(to: scrollView)
            found.append(scrollView)
          }
        }
        stack.append(contentsOf: view.subviews)
      }
      return found
    }
  }
#endif
