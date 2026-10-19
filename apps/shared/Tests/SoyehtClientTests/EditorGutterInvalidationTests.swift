#if os(macOS)
  import AppKit
  import Testing

  @testable import SoyehtClient

  /// A scroll view over a text view taller than it, as the editor lays them out.
  @MainActor private func editorViews() -> (scroll: NSScrollView, textView: NSTextView) {
    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 300, height: 400))
    let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 300, height: 4000))
    scroll.documentView = textView
    return (scroll, textView)
  }

  @MainActor @Test func scrollingTheTextRedrawsTheGutter() {
    let (scroll, textView) = editorViews()
    var redraws = 0
    let invalidation = EditorGutterInvalidation(scrollView: scroll, textView: textView) {
      redraws += 1
    }

    scroll.contentView.scroll(to: NSPoint(x: 0, y: 600))
    scroll.reflectScrolledClipView(scroll.contentView)

    #expect(redraws > 0)
    #expect(scroll.contentView.bounds.minY == 600)
    withExtendedLifetime(invalidation) {}
  }

  @MainActor @Test func aTextViewThatChangesSizeRedrawsTheGutter() {
    let (scroll, textView) = editorViews()
    var redraws = 0
    let invalidation = EditorGutterInvalidation(scrollView: scroll, textView: textView) {
      redraws += 1
    }

    textView.setFrameSize(NSSize(width: 300, height: 9000))

    #expect(redraws > 0)
    withExtendedLifetime(invalidation) {}
  }

  @MainActor @Test func aGutterThatIsGoneIsNeverToldAgain() {
    let (scroll, textView) = editorViews()
    var redraws = 0
    var invalidation: EditorGutterInvalidation? = EditorGutterInvalidation(
      scrollView: scroll, textView: textView
    ) {
      redraws += 1
    }
    scroll.contentView.scroll(to: NSPoint(x: 0, y: 100))
    let before = redraws
    #expect(before > 0)

    invalidation = nil
    scroll.contentView.scroll(to: NSPoint(x: 0, y: 900))
    textView.setFrameSize(NSSize(width: 300, height: 5000))

    #expect(redraws == before)
    #expect(invalidation == nil)
  }
#endif
