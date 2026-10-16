import AppKit
import SoyehtClient
import SwiftUI

/// What a writing text viewport's coordinator does for its scroll view.
@MainActor protocol WritingViewportCoordinator: NSObject {
  associatedtype Parent
  init(_ parent: Parent)
  var parent: Parent { get set }
  func makeScrollView() -> NSScrollView
  func update(_ scrollView: NSScrollView)
}

/// A writing text viewport: a scroll view of text that its coordinator keeps in step with the
/// document, and that tells the coordinator when it scrolls.
protocol WritingScrollViewport: NSViewRepresentable
where
  NSViewType == NSScrollView, Coordinator: WritingViewportCoordinator, Coordinator.Parent == Self
{}

extension WritingScrollViewport {
  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeNSView(context: Context) -> NSScrollView { context.coordinator.makeScrollView() }

  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(scrollView)
  }

  static func dismantleNSView(_ scrollView: NSScrollView, coordinator: Coordinator) {
    NotificationCenter.default.removeObserver(
      coordinator, name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
  }
}

extension NSScrollView {
  /// The scroll view a writing text view sits in: the face background and overlay scrollers.
  @MainActor static func writingScrollView(holding textView: NSTextView) -> NSScrollView {
    let scroll = NSScrollView()
    scroll.drawsBackground = true
    scroll.backgroundColor = NSColor(MacTheme.face)
    scroll.hasVerticalScroller = true
    scroll.autohidesScrollers = true
    scroll.scrollerStyle = .overlay
    scroll.borderType = .noBorder
    scroll.documentView = textView
    return scroll
  }
}

extension NSTextView {
  /// Paints the view and its scroll view from the active palette.
  @MainActor func applyWritingPalette() {
    enclosingScrollView?.backgroundColor = NSColor(MacTheme.face)
    backgroundColor = NSColor(MacTheme.face)
    insertionPointColor = NSColor(MacTheme.accent)
    selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
  }

  /// Moves the caret to a UTF-16 offset and scrolls it into view.
  @MainActor func placeCaret(at location: Int) {
    setSelectedRange(NSRange(location: location, length: 0))
    scrollRangeToVisible(NSRange(location: location, length: 0))
  }

  /// The editing behavior every writing text view shares.
  @MainActor func configureWritingEditing(
    delegate: NSTextViewDelegate, checksSpelling: Bool = true
  ) {
    self.delegate = delegate
    isRichText = false
    importsGraphics = false
    allowsUndo = true
    usesFindPanel = true
    if checksSpelling { isContinuousSpellCheckingEnabled = true }
    drawsBackground = true
    applyWritingPalette()
  }

  /// Lays the text out as one column that follows the width of the view.
  @MainActor func configureWritingColumn(inset: NSSize) {
    textContainerInset = inset
    minSize = NSSize(width: 0, height: 0)
    maxSize = NSSize(
      width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
    isVerticallyResizable = true
    isHorizontallyResizable = false
    autoresizingMask = [.width]
    textContainer?.widthTracksTextView = true
    textContainer?.lineFragmentPadding = 0
  }
}

extension Collection {
  /// The element at `index`, or nil when the collection has none there.
  subscript(safe index: Index) -> Element? {
    indices.contains(index) ? self[index] : nil
  }
}
