import SwiftUI

extension View {
  /// Gives every AppKit scroll view drawn inside this view's frame thin overlay scrollers that
  /// appear while scrolling and fade out. It looks again when the frame or the window changes,
  /// when the system scroller preference changes and when a scroll view loses the style, and on
  /// a content update at most once a second, so typing and streaming do no extra work. iOS
  /// scroll indicators already behave this way.
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
    /// keystroke or streamed token does not walk the view tree each time. An update inside the
    /// interval is not lost: one look follows when the interval ends.
    static let updateInterval: TimeInterval = 1
    /// A scroll view that is not built yet is looked for this often, until `findWindow` has
    /// passed. A count of looks would not do: a busy machine runs them all before SwiftUI has
    /// built the scroll view.
    static let retryInterval: TimeInterval = 0.05
    static let findWindow: TimeInterval = 10

    private var observesPreference = false
    private var pending = false
    private var lastLook = Date.distantPast
    private var findUntil = Date.distantPast
    /// The scroll views whose style this marker keeps: AppKit or SwiftUI may put the preferred
    /// style back on one at any time, without the system preference having changed.
    private let watched = NSMapTable<NSScrollView, NSKeyValueObservation>(
      keyOptions: .weakMemory, valueOptions: .strongMemory)

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
      let wait = Self.updateInterval - Date().timeIntervalSince(lastLook)
      if wait <= 0 { configureSoon() } else { look(after: wait) }
    }

    /// AppKit builds a SwiftUI scroll view's backing view after this marker, so the look waits
    /// a run-loop turn and tries again, a moment apart, while the frame holds no scroll view yet.
    func configureSoon() {
      findUntil = Date().addingTimeInterval(Self.findWindow)
      look(after: 0)
    }

    /// One look, once `delay` has passed. A look already waiting covers this one.
    private func look(after delay: TimeInterval) {
      guard !pending else { return }
      pending = true
      let work = { [weak self] in
        MainActor.assumeIsolated {
          guard let self else { return }
          self.pending = false
          self.lastLook = Date()
          if self.configure().isEmpty, Date() < self.findUntil {
            self.look(after: Self.retryInterval)
          }
        }
      }
      if delay > 0 {
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
      } else {
        RunLoop.main.perform(inModes: [.common], block: work)
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
            keep(scrollView)
            found.append(scrollView)
          }
        }
        stack.append(contentsOf: view.subviews)
      }
      return found
    }

    /// Looks again when the scroll view's style changes to anything but the overlay.
    private func keep(_ scrollView: NSScrollView) {
      guard watched.object(forKey: scrollView) == nil else { return }
      let token = scrollView.observe(\.scrollerStyle) { [weak self] scrollView, _ in
        MainActor.assumeIsolated {
          if scrollView.scrollerStyle != .overlay { self?.configureSoon() }
        }
      }
      watched.setObject(token, forKey: scrollView)
    }
  }
#endif
