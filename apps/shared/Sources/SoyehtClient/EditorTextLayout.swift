#if os(macOS)
  import AppKit

  /// Geometry of the paged code text view on the Mac. Lines never wrap, so the
  /// view grows sideways with its longest line; it must also never be
  /// narrower than the visible area. AppKit shrinks a horizontally resizable
  /// text view to its text, and a click to the right of a page of short lines
  /// then landed on the clip view instead of the text, so nothing happened.
  @MainActor public enum EditorTextLayout {
    public static func configure(_ textView: NSTextView) {
      textView.minSize = .zero
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = true
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = false
      textView.textContainer?.containerSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
    }

    /// Sizes the text view for the whole virtual document: at least the
    /// visible width, so a click past a short line's end still reaches the
    /// text view, which puts the caret at that line's end.
    public static func fit(_ textView: NSTextView, visible: NSSize, documentHeight: CGFloat) {
      let height = max(visible.height, documentHeight)
      textView.minSize = NSSize(width: visible.width, height: height)
      let width = max(visible.width, textView.frame.width)
      if abs(textView.frame.height - height) > 0.5 || textView.frame.width < visible.width {
        textView.setFrameSize(NSSize(width: width, height: height))
      }
    }
  }
#endif
