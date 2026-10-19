#if os(macOS)
  import AppKit

  /// Tells the line-number gutter to draw again whenever what it draws from moved.
  ///
  /// The gutter is a sibling of the scroll view, not part of it, so AppKit never repaints it when
  /// the text scrolls or when the text view is resized or re-fitted; before this, only a push from
  /// the editor (a page render or a caret move) did. A gutter that drew while the clip view or the
  /// text view was not yet where it settled kept that picture, with the numbers off screen or
  /// stale, until the next push. Every move of the clip view's bounds or of the text view's frame
  /// now invalidates it.
  @MainActor public final class EditorGutterInvalidation: NSObject {
    private let redraw: @MainActor () -> Void

    public init(
      scrollView: NSScrollView, textView: NSTextView, redraw: @escaping @MainActor () -> Void
    ) {
      self.redraw = redraw
      super.init()
      scrollView.contentView.postsBoundsChangedNotifications = true
      textView.postsFrameChangedNotifications = true
      let center = NotificationCenter.default
      center.addObserver(
        self, selector: #selector(moved(_:)), name: NSView.boundsDidChangeNotification,
        object: scrollView.contentView)
      center.addObserver(
        self, selector: #selector(moved(_:)), name: NSView.frameDidChangeNotification,
        object: textView)
    }

    deinit {
      NotificationCenter.default.removeObserver(self)
    }

    @objc private func moved(_ notification: Notification) {
      redraw()
    }
  }
#endif
