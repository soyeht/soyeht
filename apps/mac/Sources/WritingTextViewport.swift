import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingTextViewport: WritingScrollViewport {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  @Binding var selection: SoyehtProtocol.TextRange?
  @Binding var selectionGeometry: PaneLayoutRect?
  @Binding var activeLine: UInt32
  let onEditingActivity: () -> Void
  let onFormatShortcut: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate, WritingViewportCoordinator {
    var parent: WritingTextViewport
    private let textView = WritingImagePasteTextView()
    private var text = WritingViewportText()
    private var renderedRevision: String?
    private var renderedFont: WritingFont?
    private var renderedTheme: TerminalColorTheme?
    private var rendering = false
    private weak var scrollView: NSScrollView?

    init(_ parent: WritingTextViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView.writingScrollView(holding: textView)
      scroll.contentView.postsBoundsChangedNotifications = true
      NotificationCenter.default.addObserver(
        self, selector: #selector(viewportDidScroll(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      scrollView = scroll
      configureTextView()
      textView.writingFormatShortcutHandler = { [weak self] kind in
        self?.applyFormatShortcut(kind)
      }
      return scroll
    }

    func update(_ scrollView: NSScrollView) {
      guard let viewport = parent.view.viewport else { return }
      textView.applyWritingPalette()
      let next = WritingViewportText(lines: viewport.lines, firstLine: viewport.fromLine)
      let revision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if revision != renderedRevision || renderedFont != parent.document.font
        || renderedTheme != SoyehtThemeStore.shared.activeTheme
      {
        text = next
        renderedRevision = revision
        renderedFont = parent.document.font
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: text.clamped(textView.selectedRange()))
      } else if parent.activeLine != currentLine() || parent.document.focusMode != focusApplied {
        render(selection: text.clamped(textView.selectedRange()))
      }
      publishSelectionGeometry(deferred: true)
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !rendering else { return }
      let range = textView.selectedRange()
      guard let start = text.position(utf16Offset: range.location),
        let end = text.position(utf16Offset: NSMaxRange(range))
      else { return }
      parent.selection = SoyehtProtocol.TextRange(start: start, end: end)
      publishSelectionGeometry()
      if parent.activeLine != start.line {
        parent.activeLine = start.line
        render(selection: range)
      }
    }

    func textView(
      _ textView: NSTextView, shouldChangeTextIn affectedCharRange: NSRange,
      replacementString: String?
    ) -> Bool {
      let replacement = replacementString ?? ""
      guard let change = text.replace(affectedCharRange, with: replacement) else { return false }
      parent.onEditingActivity()
      let cursorPosition = text.position(utf16Offset: change.cursor) ?? change.edit.range.start
      parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: [change.edit])
      parent.activeLine = cursorPosition.line
      parent.selection = SoyehtProtocol.TextRange(start: cursorPosition, end: cursorPosition)
      parent.selectionGeometry = nil
      render(selection: NSRange(location: change.cursor, length: 0))
      return false
    }

    private var focusApplied: Bool {
      guard let storage = textView.textStorage, storage.length > 0 else { return false }
      return storage.attribute(.writingFocus, at: 0, effectiveRange: nil) as? Bool ?? false
    }

    private func currentLine() -> UInt32 {
      text.line(utf16Offset: textView.selectedRange().location)
    }

    private func render(selection: NSRange) {
      rendering = true
      let projection = WritingMarkdownProjection.render(
        source: text.source, firstLine: text.firstLine, activeLine: parent.activeLine)
      let attributed = WritingTextStyle.attributed(
        projection, font: parent.document.font, focus: parent.document.focusMode)
      textView.textStorage?.setAttributedString(attributed)
      textView.setSelectedRange(text.clamped(selection))
      textView.needsDisplay = true
      rendering = false
      publishSelectionGeometry(deferred: true)
    }

    @objc private func viewportDidScroll(_ notification: Notification) {
      publishSelectionGeometry()
    }

    private func publishSelectionGeometry(deferred: Bool = false) {
      if deferred {
        Task { @MainActor [weak self] in self?.publishSelectionGeometry() }
        return
      }
      guard scrollView != nil else {
        parent.selectionGeometry = nil
        return
      }
      let selectedRange = textView.selectedRange()
      guard selectedRange.length > 0, let layoutManager = textView.layoutManager,
        let textContainer = textView.textContainer
      else {
        parent.selectionGeometry = nil
        return
      }
      let glyphRange = layoutManager.glyphRange(
        forCharacterRange: selectedRange, actualCharacterRange: nil)
      var rect = layoutManager.boundingRect(forGlyphRange: glyphRange, in: textContainer)
      rect.origin.x += textView.textContainerOrigin.x
      rect.origin.y += textView.textContainerOrigin.y
      let visible = textView.visibleRect
      let clipped = rect.intersection(visible)
      guard !clipped.isNull, clipped.width > 0, clipped.height > 0 else {
        parent.selectionGeometry = nil
        return
      }
      parent.selectionGeometry = PaneLayoutRect(
        origin: PaneLayoutPoint(
          x: clipped.minX - visible.minX,
          y: clipped.minY - visible.minY),
        size: PaneLayoutSize(width: clipped.width, height: clipped.height))
    }

    private func applyFormatShortcut(_ kind: WritingFormatKind) {
      let selectedRange = textView.selectedRange()
      guard selectedRange.length > 0,
        let start = text.position(utf16Offset: selectedRange.location),
        let end = text.position(utf16Offset: NSMaxRange(selectedRange))
      else { return }
      parent.onFormatShortcut(
        kind, SoyehtProtocol.TextRange(start: start, end: end))
    }

    private func configureTextView() {
      textView.configureWritingEditing(delegate: self)
      // The pure writing plan owns both the 66-character cap and the page inset.
      textView.configureWritingColumn(inset: .zero)
      textView.acceptImages { [weak self] pasteboard in
        guard let self else { return false }
        return WritingImageInsertion.insert(
          from: pasteboard, in: textView, text: text, client: parent.client,
          fileId: parent.view.file.fileId)
      }
    }
  }
}

@MainActor private enum WritingTextStyle {
  static func attributed(
    _ projection: WritingMarkdownProjection, font: WritingFont, focus: Bool
  ) -> NSAttributedString {
    let base = MacTheme.proseNSFont(font, PaneLayoutContentMetrics.textFontPointSize)
    let paragraph = paragraphStyle(lineHeight: PaneLayoutContentMetrics.textLineHeight)
    let result = NSMutableAttributedString(
      string: projection.text,
      attributes: [
        .font: base, .foregroundColor: NSColor(MacTheme.ink), .paragraphStyle: paragraph,
        .writingFocus: focus,
      ])
    for line in projection.lines { style(line, in: result, font: font, focus: focus) }
    for span in projection.spans { style(span, in: result, base: base) }
    return result
  }

  private static func style(
    _ line: WritingMarkdownLine, in result: NSMutableAttributedString,
    font: WritingFont, focus: Bool
  ) {
    guard line.range.length > 0 else { return }
    let color = focus && !line.rawSyntax ? NSColor(MacTheme.muted) : NSColor(MacTheme.ink)
    result.addAttribute(.foregroundColor, value: color, range: line.range)
    switch line.kind {
    case .heading(let level):
      let size: CGFloat = level == 1 ? 34 : (level == 2 ? 28 : 23)
      result.addAttributes(
        [
          .font: MacTheme.proseNSFont(font, size, bold: true),
          .paragraphStyle: paragraphStyle(lineHeight: size + 15),
        ],
        range: line.range)
    case .listItem(let depth, _):
      result.addAttribute(
        .paragraphStyle, value: indented(22 + CGFloat(depth * 20)), range: line.range)
    case .quote:
      result.addAttributes(
        [.foregroundColor: NSColor(MacTheme.secondary), .paragraphStyle: indented(28)],
        range: line.range)
    case .codeFence:
      result.addAttributes(
        [
          .font: MacTheme.monoNSFont(11), .foregroundColor: NSColor(MacTheme.muted),
          .paragraphStyle: paragraphStyle(lineHeight: 12),
        ],
        range: line.range)
    case .code:
      result.addAttributes(
        [
          .font: MacTheme.monoNSFont(13), .backgroundColor: NSColor(MacTheme.well),
          .paragraphStyle: indented(18),
        ],
        range: line.range)
    case .table:
      result.addAttributes(
        [
          .font: MacTheme.monoNSFont(13), .foregroundColor: color,
          .paragraphStyle: tableStyle(), .backgroundColor: NSColor(MacTheme.well),
        ], range: line.range)
    case .image:
      result.addAttributes([.foregroundColor: NSColor(MacTheme.accent)], range: line.range)
    case .blank, .paragraph:
      break
    }
  }

  private static func style(
    _ span: WritingMarkdownSpan, in result: NSMutableAttributedString, base: NSFont
  ) {
    let range = NSRange(location: span.location, length: span.length)
    guard NSMaxRange(range) <= result.length else { return }
    switch span.kind {
    case .strong:
      result.addAttribute(
        .font, value: NSFontManager.shared.convert(base, toHaveTrait: .boldFontMask), range: range)
    case .emphasis:
      result.addAttribute(
        .font, value: NSFontManager.shared.convert(base, toHaveTrait: .italicFontMask), range: range
      )
    case .inlineCode:
      result.addAttributes(
        [.font: MacTheme.monoNSFont(14), .backgroundColor: NSColor(MacTheme.well)], range: range)
    case .link:
      result.addAttributes(
        [
          .foregroundColor: NSColor(MacTheme.accent),
          .underlineStyle: NSUnderlineStyle.single.rawValue,
        ], range: range)
    case .syntax:
      result.addAttributes(
        [.font: MacTheme.monoNSFont(13), .foregroundColor: NSColor(MacTheme.muted)], range: range)
    }
  }

  private static func paragraphStyle(lineHeight: CGFloat) -> NSParagraphStyle {
    let style = NSMutableParagraphStyle()
    style.minimumLineHeight = lineHeight
    style.maximumLineHeight = lineHeight
    style.paragraphSpacing = 10
    return style
  }

  private static func indented(_ value: CGFloat) -> NSParagraphStyle {
    let style =
      paragraphStyle(lineHeight: 31) as? NSMutableParagraphStyle ?? NSMutableParagraphStyle()
    style.firstLineHeadIndent = value
    style.headIndent = value
    return style
  }

  private static func tableStyle() -> NSParagraphStyle {
    let style = NSMutableParagraphStyle()
    style.minimumLineHeight = 27
    style.maximumLineHeight = 27
    style.paragraphSpacing = 0
    style.headIndent = 12
    style.firstLineHeadIndent = 12
    style.tailIndent = -12
    style.defaultTabInterval = 180
    style.tabStops = stride(from: 180, through: 1_080, by: 180).map {
      NSTextTab(textAlignment: .left, location: CGFloat($0), options: [:])
    }
    return style
  }
}

extension NSAttributedString.Key {
  fileprivate static let writingFocus = NSAttributedString.Key("soyeht.writing.focus")
}
