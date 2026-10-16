import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct ScreenplayViewport: WritingScrollViewport {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  let structure: WritingStructureResult
  let pageSize: PaneLayoutSize
  let scale: Double
  @Binding var activeLine: UInt32
  @Binding var targetLine: UInt32?
  @Binding var editRequest: ScreenplayEditRequest?
  @Binding var hoveredLine: UInt32?
  @Binding var hoverY: CGFloat
  @Binding var elementMenuAnchor: PaneLayoutPoint?
  @Binding var showingElementMenu: Bool
  @Binding var smartTypeKind: ScreenplayElementKind
  @Binding var smartTypeSuggestions: [String]
  let onEditingActivity: () -> Void

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate, WritingViewportCoordinator {
    var parent: ScreenplayViewport
    private let textView = ScreenplayTextView()
    private var text = WritingViewportText()
    private var renderedRevision: String?
    private var renderedTheme: TerminalColorTheme?
    private var applying = false
    private var appliedRequest: UUID?
    private weak var scrollView: NSScrollView?

    init(_ parent: ScreenplayViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView.writingScrollView(holding: textView)
      configure(scroll)
      NotificationCenter.default.addObserver(
        self, selector: #selector(viewportDidScroll(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      scrollView = scroll
      applyPageGeometry(scroll)
      return scroll
    }

    func update(_ scrollView: NSScrollView) {
      guard let viewport = parent.view.viewport else { return }
      textView.applyWritingPalette()
      applyPageGeometry(scrollView)
      let next = WritingViewportText(lines: viewport.lines, firstLine: viewport.fromLine)
      let revision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if revision != renderedRevision || renderedTheme != SoyehtThemeStore.shared.activeTheme {
        text = next
        renderedRevision = revision
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: text.clamped(textView.selectedRange()))
      } else if parent.activeLine != currentLine() || focusApplied != parent.document.focusMode {
        restyle(selection: text.clamped(textView.selectedRange()))
      }
      if let request = parent.editRequest, request.id != appliedRequest {
        appliedRequest = request.id
        // Consume before applying so a remounted viewport can never replay this request.
        parent.editRequest = nil
        if ScreenplayInteraction.editRequestMatchesViewport(
          requestFileId: request.fileId, requestRevision: request.revision,
          viewportFileId: parent.view.file.fileId,
          viewportRevision: parent.view.file.revision)
        {
          apply(request)
        }
      }
      if let line = parent.targetLine, line >= viewport.fromLine, line < viewport.toLine {
        scroll(to: line)
        parent.targetLine = nil
      }
      textView.hoverHandler = { [weak self, weak scrollView] point in
        guard let self, let scrollView else { return }
        hover(at: point, scroll: scrollView)
      }
      publishElementMenuAnchor(deferred: true)
    }

    private func applyPageGeometry(_ scrollView: NSScrollView) {
      let page = NSSize(width: parent.pageSize.width, height: parent.pageSize.height)
      let currentHeight = max(page.height, textView.frame.height)
      if abs(textView.frame.width - page.width) > 0.25
        || abs(textView.frame.height - currentHeight) > 0.25
      {
        textView.setFrameSize(NSSize(width: page.width, height: currentHeight))
      }
      textView.minSize = page
      textView.maxSize = NSSize(width: page.width, height: .greatestFiniteMagnitude)
      if let container = textView.textContainer {
        let textWidth = max(0, page.width - 2 * textView.textContainerInset.width)
        let containerSize = NSSize(width: textWidth, height: .greatestFiniteMagnitude)
        if abs(container.containerSize.width - containerSize.width) > 0.25
          || container.containerSize.height != containerSize.height
        {
          container.containerSize = containerSize
        }
      }

      let scale = CGFloat(parent.scale)
      scrollView.allowsMagnification = true
      scrollView.minMagnification = 0.1
      scrollView.maxMagnification = 1
      if abs(scrollView.magnification - scale) > 0.000_1 {
        scrollView.magnification = scale
      }
      scrollView.minMagnification = scale
      scrollView.maxMagnification = scale
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !applying, let position = text.position(utf16Offset: textView.selectedRange().location)
      else { return }
      if parent.activeLine != position.line {
        parent.activeLine = position.line
        restyle(selection: textView.selectedRange())
      }
      publishElementMenuAnchor()
    }

    func textView(
      _ textView: NSTextView, shouldChangeTextIn range: NSRange, replacementString: String?
    ) -> Bool {
      replace(range, with: replacementString ?? "")
      return false
    }

    func textView(_ textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
      if commandSelector == #selector(NSResponder.insertTab(_:)) {
        advance(tab: true)
        return true
      }
      if commandSelector == #selector(NSResponder.insertNewline(_:)) {
        advance(tab: false)
        return true
      }
      return false
    }

    private var focusApplied: Bool {
      guard let storage = textView.textStorage, storage.length > 0 else { return false }
      return storage.attribute(.screenplayFocus, at: 0, effectiveRange: nil) as? Bool ?? false
    }

    private func currentLine() -> UInt32 {
      text.line(utf16Offset: textView.selectedRange().location)
    }

    private func advance(tab: Bool) {
      let line = currentLine()
      let kind = ScreenplayInteraction.element(at: line, in: parent.structure)?.kind ?? .action
      guard
        let edit = ScreenplayInteraction.advanceEdit(
          source: text.source, cursor: textView.selectedRange().location, kind: kind, tab: tab)
      else { return }
      replace(edit.range, with: edit.replacement)
    }

    private func apply(_ request: ScreenplayEditRequest) {
      replaceCurrentLine(with: request.text, as: request.kind)
    }

    private func replaceCurrentLine(with newText: String?, as kind: ScreenplayElementKind) {
      let contentRange = text.contentRange(atLocation: textView.selectedRange().location)
      let current = (text.source as NSString).substring(with: contentRange)
      let formatted = ScreenplayInteraction.formattedLine(newText ?? current, as: kind)
      replace(contentRange, with: formatted)
    }

    private func replace(_ range: NSRange, with replacement: String) {
      guard
        let edit = ScreenplayInteraction.typingEdit(
          source: text.source, firstLine: text.firstLine, range: range, replacement: replacement)
      else { return }
      parent.onEditingActivity()
      text.source = edit.source
      applyLocalReplacement(range, with: replacement)
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId, edits: [edit.wireEdit])
      parent.activeLine = edit.activeLine
      restyle(selection: NSRange(location: edit.cursor, length: 0))
    }

    private func applyLocalReplacement(_ range: NSRange, with replacement: String) {
      guard let storage = textView.textStorage, NSMaxRange(range) <= storage.length else { return }
      applying = true
      storage.replaceCharacters(in: range, with: replacement)
      applying = false
    }

    private func render(selection: NSRange) {
      applying = true
      textView.textStorage?.setAttributedString(
        ScreenplayStyle.attributed(
          text, activeLine: parent.activeLine,
          focus: parent.document.focusMode, structure: parent.structure))
      textView.setSelectedRange(text.clamped(selection))
      ensurePageLayout()
      textView.needsDisplay = true
      applying = false
      publishElementMenuAnchor(deferred: true)
    }

    private func restyle(selection: NSRange) {
      guard let storage = textView.textStorage, storage.string == text.source else {
        render(selection: selection)
        return
      }
      let styled = ScreenplayStyle.attributed(
        text, activeLine: parent.activeLine,
        focus: parent.document.focusMode, structure: parent.structure)
      applying = true
      storage.beginEditing()
      if storage.length > 0 {
        storage.setAttributes([:], range: NSRange(location: 0, length: storage.length))
        styled.enumerateAttributes(
          in: NSRange(location: 0, length: styled.length), options: []
        ) { attributes, range, _ in
          storage.setAttributes(attributes, range: range)
        }
      }
      storage.endEditing()
      textView.setSelectedRange(text.clamped(selection))
      ensurePageLayout()
      textView.needsDisplay = true
      applying = false
      publishElementMenuAnchor(deferred: true)
    }

    private func ensurePageLayout() {
      guard let layout = textView.layoutManager, let container = textView.textContainer else {
        return
      }
      if let storage = textView.textStorage, storage.length > 0 {
        layout.invalidateLayout(
          forCharacterRange: NSRange(location: 0, length: storage.length),
          actualCharacterRange: nil)
      }
      layout.ensureLayout(for: container)
    }

    private func hover(at point: NSPoint, scroll: NSScrollView) {
      guard let container = textView.textContainer, let layout = textView.layoutManager else {
        return
      }
      let containerPoint = NSPoint(
        x: point.x - textView.textContainerOrigin.x,
        y: point.y - textView.textContainerOrigin.y)
      guard layout.usedRect(for: container).contains(containerPoint) else { return }
      let glyph = layout.glyphIndex(for: containerPoint, in: container)
      let character = layout.characterIndexForGlyph(at: glyph)
      guard let position = text.position(utf16Offset: character),
        ScreenplayInteraction.variants(at: position.line, in: parent.structure) != nil
      else {
        parent.hoveredLine = nil
        return
      }
      let rect = layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
      parent.hoveredLine = position.line
      parent.hoverY =
        rect.midY + textView.textContainerOrigin.y - scroll.contentView.bounds.origin.y
    }

    private func scroll(to line: UInt32) {
      guard let location = text.utf16Location(ofLine: line) else { return }
      textView.placeCaret(at: location)
      parent.activeLine = line
      publishElementMenuAnchor(deferred: true)
    }

    @objc private func viewportDidScroll(_ notification: Notification) {
      publishElementMenuAnchor()
    }

    private func publishElementMenuAnchor(deferred: Bool = false) {
      if deferred {
        Task { @MainActor [weak self] in self?.publishElementMenuAnchor() }
        return
      }
      guard let scrollView else {
        hideInlineControls()
        return
      }
      let selection = textView.selectedRange()
      let raw = text.source as NSString
      guard selection.location != NSNotFound, selection.location <= raw.length else {
        hideInlineControls()
        return
      }
      let contentRange = text.contentRange(atLocation: selection.location)
      let lineText = raw.substring(with: contentRange)
      let showingElementMenu = MacPaneLayoutMetrics.chromePolicy.showsScreenplayElementMenu(
        lineText: lineText, selectionLength: selection.length)
      let smartTypeKind = ScreenplayInteraction.suggestionKind(
        at: text.position(utf16Offset: selection.location)?.line ?? parent.activeLine,
        in: parent.structure)
      let smartTypeSuggestions =
        showingElementMenu
        ? Array(
          ScreenplayInteraction.suggestions(
            for: smartTypeKind, prefix: lineText, in: parent.structure
          ).prefix(5))
        : []
      guard showingElementMenu || !smartTypeSuggestions.isEmpty,
        let layout = textView.layoutManager, let container = textView.textContainer
      else {
        hideInlineControls()
        return
      }
      layout.ensureLayout(for: container)
      let lineRect: NSRect
      if raw.length == 0 || (selection.location == raw.length && contentRange.length == 0) {
        lineRect = layout.extraLineFragmentRect
      } else {
        let character = min(selection.location, max(0, raw.length - 1))
        let glyph = layout.glyphIndexForCharacter(at: character)
        lineRect = layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
      }
      let visible = scrollView.documentVisibleRect
      let scale = CGFloat(parent.scale)
      let pageWidth = CGFloat(parent.pageSize.width) * scale
      let caretX =
        (lineRect.minX + textView.textContainerOrigin.x - visible.minX) * scale
      let controlInset = CGFloat(PaneLayoutContentMetrics.screenplayInlineElementMenuInset)
      let controlGap = CGFloat(PaneLayoutContentMetrics.screenplayInlineControlGap)
      let menuWidth = CGFloat(PaneLayoutContentMetrics.screenplayInlineElementMenuWidth)
      let maximumStart = max(controlInset, pageWidth - menuWidth - controlInset)
      let controlStart = min(max(controlInset, caretX + controlGap), maximumStart)
      let y =
        (lineRect.midY + textView.textContainerOrigin.y - visible.minY) * scale
        + CGFloat(PaneLayoutContentMetrics.screenplayInlineAnchorClearance)
      let pageHeight = CGFloat(parent.pageSize.height) * scale
      guard y >= 0, y <= pageHeight else {
        hideInlineControls()
        return
      }
      parent.showingElementMenu = showingElementMenu
      parent.smartTypeKind = smartTypeKind
      parent.smartTypeSuggestions = smartTypeSuggestions
      parent.elementMenuAnchor = PaneLayoutPoint(
        x: controlStart,
        y: y)
    }

    private func hideInlineControls() {
      parent.elementMenuAnchor = nil
      parent.showingElementMenu = false
      parent.smartTypeSuggestions = []
    }

    private func configure(_ scroll: NSScrollView) {
      textView.configureWritingEditing(delegate: self)
      textView.textContainerInset = NSSize(
        width: PaneLayoutContentMetrics.screenplayPageHorizontalInset, height: 58)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = false
      // The fixed page owns its text width. Letting the scroll view autoresize the document view
      // makes the first layout use the visible (scaled) width until a later interaction updates it.
      textView.autoresizingMask = []
      textView.textContainer?.widthTracksTextView = false
      textView.textContainer?.lineFragmentPadding = 0
      scroll.contentView.postsBoundsChangedNotifications = true
    }
  }
}

@MainActor private final class ScreenplayTextView: NSTextView {
  var hoverHandler: ((NSPoint) -> Void)?
  private var hoverArea: NSTrackingArea?

  override func updateTrackingAreas() {
    if let hoverArea { removeTrackingArea(hoverArea) }
    let area = NSTrackingArea(
      rect: bounds, options: [.activeInKeyWindow, .inVisibleRect, .mouseMoved], owner: self)
    addTrackingArea(area)
    hoverArea = area
    super.updateTrackingAreas()
  }

  override func mouseMoved(with event: NSEvent) {
    hoverHandler?(convert(event.locationInWindow, from: nil))
    super.mouseMoved(with: event)
  }
}

@MainActor private enum ScreenplayStyle {
  static func attributed(
    _ text: WritingViewportText, activeLine: UInt32, focus: Bool,
    structure: WritingStructureResult
  ) -> NSAttributedString {
    let result = NSMutableAttributedString(
      string: text.source,
      attributes: [
        .font: MacTheme.screenplayNSFont(PaneLayoutContentMetrics.screenplayFontPointSize),
        .foregroundColor: NSColor(MacTheme.ink),
        .paragraphStyle: ScreenplayLineStyle.basePageParagraph(), .screenplayFocus: focus,
      ])
    let appearance = ScreenplayPageAppearance(
      boldFont: MacTheme.screenplayNSFont(
        PaneLayoutContentMetrics.screenplayFontPointSize, bold: true),
      secondaryInk: NSColor(MacTheme.secondary),
      hiddenMarkerFont: MacTheme.screenplayNSFont(ScreenplayLineStyle.hiddenMarkerFontSize),
      hiddenMarkerInk: NSColor(MacTheme.face))
    for (kind, range) in text.elementRanges(in: structure) {
      let line = ScreenplayLineStyle(
        kind: kind, line: result.attributedSubstring(from: range).string)
      line.apply(to: result, range: range, appearance: appearance)
    }
    for range in text.variantRanges(in: structure) {
      result.addAttributes(
        [
          .underlineStyle: NSUnderlineStyle.single.union(.patternDot).rawValue,
          .underlineColor: NSColor(MacTheme.accent),
        ], range: range)
    }
    if focus {
      for range in text.inactiveLineRanges(activeLine: activeLine) {
        result.addAttribute(.foregroundColor, value: NSColor(MacTheme.muted), range: range)
      }
    }
    return result
  }
}

extension NSAttributedString.Key {
  fileprivate static let screenplayFocus = NSAttributedString.Key("soyeht.screenplay.focus")
}
