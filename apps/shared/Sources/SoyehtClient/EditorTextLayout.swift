#if os(macOS)
  import AppKit

  /// Geometry of the paged code text view on the Mac.
  ///
  /// Without soft wrap, lines never wrap, so the view grows sideways with its longest line; it
  /// must also never be narrower than the visible area. AppKit shrinks a horizontally resizable
  /// text view to its text, and a click to the right of a page of short lines then landed on
  /// the clip view instead of the text, so nothing happened.
  ///
  /// With soft wrap the view is exactly as wide as the visible area and its text container
  /// holds `columns` cells, so AppKit breaks lines where `EditorWrapLayout` does.
  @MainActor public enum EditorTextLayout {
    public static func configure(
      _ textView: NSTextView, wrapColumns: Int? = nil, characterWidth: CGFloat = 0
    ) {
      textView.minSize = .zero
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = wrapColumns == nil
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = false
      let padding = textView.textContainer?.lineFragmentPadding ?? 0
      // A quarter point of slack keeps the last cell from wrapping on rounding, and is far
      // narrower than one more cell.
      let width =
        wrapColumns.map { CGFloat($0) * characterWidth + 2 * padding + 0.25 }
        ?? CGFloat.greatestFiniteMagnitude
      textView.textContainer?.containerSize = NSSize(
        width: width, height: CGFloat.greatestFiniteMagnitude)
    }

    /// Puts `head` before the text and keeps every selection on the text it was on. The text
    /// after it keeps its attributes; nothing reaches the delegate, as with a page render.
    public static func prepend(_ head: NSAttributedString, to textView: NSTextView) {
      guard let storage = textView.textStorage, head.length > 0 else { return }
      let selections = textView.selectedRanges.map(\.rangeValue)
      storage.beginEditing()
      storage.insert(head, at: 0)
      storage.endEditing()
      textView.selectedRanges = selections.map {
        NSValue(range: NSRange(location: $0.location + head.length, length: $0.length))
      }
    }

    /// The paragraph style of code: fixed line height, tab stops every
    /// `EditorWrapLayout.tabWidth` cells and, with soft wrap, breaks between any two characters.
    public static func paragraphStyle(
      lineHeight: CGFloat, characterWidth: CGFloat, wraps: Bool
    ) -> NSParagraphStyle {
      let paragraph = NSMutableParagraphStyle()
      paragraph.minimumLineHeight = lineHeight
      paragraph.maximumLineHeight = lineHeight
      paragraph.tabStops = []
      paragraph.defaultTabInterval = CGFloat(EditorWrapLayout.tabWidth) * characterWidth
      paragraph.lineBreakMode = wraps ? .byCharWrapping : .byClipping
      return paragraph
    }

    /// Cells per row for a text view whose visible area is `visibleWidth` points wide.
    public static func wrapColumns(
      _ textView: NSTextView, visibleWidth: CGFloat, characterWidth: CGFloat
    ) -> Int {
      let padding = textView.textContainer?.lineFragmentPadding ?? 0
      return EditorWrapLayout.columns(
        fitting: Double(visibleWidth - 2 * textView.textContainerInset.width - 2 * padding),
        characterWidth: Double(characterWidth))
    }

    /// Sizes the text view for the whole virtual document: at least the
    /// visible width, so a click past a short line's end still reaches the
    /// text view, which puts the caret at that line's end. With soft wrap it
    /// is exactly the visible width, so nothing scrolls sideways.
    public static func fit(_ textView: NSTextView, visible: NSSize, documentHeight: CGFloat) {
      let height = max(visible.height, documentHeight)
      textView.minSize = NSSize(width: visible.width, height: height)
      let wraps = !textView.isHorizontallyResizable
      let width = wraps ? visible.width : max(visible.width, textView.frame.width)
      if abs(textView.frame.height - height) > 0.5 || textView.frame.width < visible.width
        || (wraps && abs(textView.frame.width - width) > 0.5)
      {
        textView.setFrameSize(NSSize(width: width, height: height))
      }
    }
  }
#endif
