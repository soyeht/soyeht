import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

/// A fixed Courier page. The outer scroll view owns pinch and bounded pan; the text
/// view owns vertical screenplay scrolling only while the whole page is fitted.
@MainActor
struct PhoneWritingScreenplaySurface: UIViewRepresentable {
  @Bindable var client: EngineClient
  let identity: PhoneWritingIdentity
  let writing: WritingDocumentView
  let file: EditorFileView
  let structure: WritingStructureResult
  let viewportSize: PaneLayoutSize
  let pageSize: PaneLayoutSize
  let fitScale: Double
  let zoomMultiplier: Double
  let accessibilityName: String
  let onEditingActivity: () -> Void
  let onSelectionActivity: (Bool) -> Void
  let onScrollActivity: () -> Void
  let onZoomChanged: (Double) -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeUIView(context: Context) -> UIScrollView {
    context.coordinator.makeScrollView()
  }

  func updateUIView(_ scrollView: UIScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(scrollView)
  }

  @MainActor final class Coordinator: NSObject, UIScrollViewDelegate, UITextViewDelegate {
    var parent: PhoneWritingScreenplaySurface
    private let pageView = UIView()
    private let textView = UITextView()
    private let elementButton = UIButton(type: .system)
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedIdentity: String?
    private var applying = false

    init(_ parent: PhoneWritingScreenplaySurface) {
      self.parent = parent
    }

    func makeScrollView() -> UIScrollView {
      let scrollView = UIScrollView()
      scrollView.delegate = self
      scrollView.backgroundColor = UIColor(PhonePalette.face)
      scrollView.bouncesZoom = true
      scrollView.decelerationRate = .normal
      scrollView.keyboardDismissMode = .interactive
      scrollView.showsHorizontalScrollIndicator = true
      scrollView.showsVerticalScrollIndicator = true
      scrollView.indicatorStyle = .default
      scrollView.accessibilityIdentifier =
        "phone-writing-screenplay-\(parent.identity.paneId)"

      pageView.backgroundColor = UIColor(PhonePalette.face)
      pageView.layer.cornerRadius = 12
      pageView.layer.borderColor = UIColor(PhonePalette.secondary).withAlphaComponent(0.16).cgColor
      pageView.layer.borderWidth = 1 / UIScreen.main.scale

      textView.delegate = self
      textView.backgroundColor = UIColor(PhonePalette.face)
      textView.isEditable = true
      textView.isSelectable = true
      textView.alwaysBounceVertical = true
      textView.keyboardDismissMode = .interactive
      textView.autocorrectionType = .yes
      textView.spellCheckingType = .yes
      textView.textContainer.lineFragmentPadding = 0
      textView.textContainerInset = UIEdgeInsets(
        top: 58,
        left: PaneLayoutContentMetrics.screenplayPageHorizontalInset,
        bottom: 58,
        right: PaneLayoutContentMetrics.screenplayPageHorizontalInset)
      textView.layer.cornerRadius = 12
      textView.clipsToBounds = true
      textView.showsHorizontalScrollIndicator = false
      textView.showsVerticalScrollIndicator = true
      textView.indicatorStyle = .default
      textView.accessibilityLabel = "\(parent.accessibilityName), screenplay"

      var configuration = UIButton.Configuration.plain()
      configuration.image = UIImage(systemName: "plus.circle")
      configuration.contentInsets = .zero
      elementButton.configuration = configuration
      elementButton.frame.size = CGSize(
        width: PhoneWritingLayoutMetrics.standard.minimumHitTarget,
        height: PhoneWritingLayoutMetrics.standard.minimumHitTarget)
      elementButton.showsMenuAsPrimaryAction = true
      elementButton.accessibilityLabel = "Screenplay element and SmartType"
      elementButton.accessibilityIdentifier =
        "phone-writing-screenplay-element-\(parent.identity.paneId)"
      elementButton.isHidden = true

      textView.addSubview(elementButton)
      pageView.addSubview(textView)
      scrollView.addSubview(pageView)
      update(scrollView)
      return scrollView
    }

    func update(_ scrollView: UIScrollView) {
      let page = CGSize(width: parent.pageSize.width, height: parent.pageSize.height)
      if pageView.bounds.size != page {
        pageView.frame = CGRect(origin: .zero, size: page)
        textView.frame = pageView.bounds
        scrollView.contentSize = page
      }

      let minimum = CGFloat(parent.fitScale)
      let maximum = minimum * CGFloat(PhoneWritingLayoutMetrics.standard.maximumScreenplayZoom)
      let desired = min(maximum, max(minimum, minimum * CGFloat(parent.zoomMultiplier)))
      scrollView.minimumZoomScale = minimum
      scrollView.maximumZoomScale = max(minimum, maximum)
      if abs(scrollView.zoomScale - desired) > 0.000_1 {
        scrollView.setZoomScale(desired, animated: false)
      }
      centerPage(in: scrollView)
      updateTextScrolling(scrollView)
      updateAccessibilityValue(scale: desired)

      guard let viewport = parent.file.viewport else { return }
      firstLine = viewport.fromLine
      let identity = [
        viewport.file.revision,
        String(viewport.fromLine),
        String(viewport.toLine),
        parent.writing.document.sidecarRevision,
        String(describing: SoyehtThemeStore.shared.activeTheme),
      ].joined(separator: ":")
      guard identity != renderedIdentity else { return }
      source = EditorEditing.viewportText(viewport.lines)
      renderedIdentity = identity
      render(selection: valid(textView.selectedRange))
      updateElementMenu()
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? {
      pageView
    }

    func scrollViewDidZoom(_ scrollView: UIScrollView) {
      centerPage(in: scrollView)
      updateTextScrolling(scrollView)
    }

    func scrollViewDidEndZooming(
      _ scrollView: UIScrollView, with view: UIView?, atScale scale: CGFloat
    ) {
      let minimum = max(scrollView.minimumZoomScale, 0.000_1)
      updateAccessibilityValue(scale: scale)
      parent.onZoomChanged(Double(scale / minimum))
    }

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
      parent.onScrollActivity()
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
      guard scrollView === textView else { return }
      parent.onScrollActivity()
      updateElementMenu()
    }

    func textViewDidChangeSelection(_ textView: UITextView) {
      guard !applying else { return }
      parent.onSelectionActivity(textView.selectedRange.length > 0)
      updateElementMenu()
    }

    func textView(
      _ textView: UITextView, shouldChangeTextIn range: NSRange,
      replacementText text: String
    ) -> Bool {
      guard !applying,
        let edit = ScreenplayInteraction.typingEdit(
          source: source, firstLine: firstLine, range: range, replacement: text)
      else { return false }
      source = edit.source
      parent.client.enqueueEditorEdits(
        fileId: parent.file.file.fileId, edits: [edit.wireEdit])
      render(selection: NSRange(location: edit.cursor, length: 0))
      parent.onEditingActivity()
      updateElementMenu()
      return false
    }

    private func updateElementMenu() {
      let selection = textView.selectedRange
      guard selection.location != NSNotFound, selection.length == 0,
        selection.location <= source.utf16.count
      else {
        elementButton.isHidden = true
        return
      }
      let raw = source as NSString
      let lineRange = raw.lineRange(for: selection)
      var contentRange = lineRange
      while contentRange.length > 0 {
        let character = raw.character(at: NSMaxRange(contentRange) - 1)
        guard character == 10 || character == 13 else { break }
        contentRange.length -= 1
      }
      let lineText = raw.substring(with: contentRange)
      guard WritingChromePolicy().showsScreenplayElementMenu(
        lineText: lineText, selectionLength: selection.length)
      else {
        elementButton.isHidden = true
        return
      }
      let activeLine = lineNumber(at: selection.location)
      let suggestionKind = ScreenplayInteraction.suggestionKind(
        at: activeLine, in: parent.structure)
      let suggestions = Array(
        ScreenplayInteraction.suggestions(
          for: suggestionKind, prefix: lineText, in: parent.structure).prefix(5))
      let elementActions = screenplayElementKinds.map { kind in
        UIAction(title: elementTitle(kind)) { [weak self] _ in
          self?.applyLine(kind: kind, value: "", range: contentRange)
        }
      }
      var children: [UIMenuElement] = [
        UIMenu(title: "Element", options: .displayInline, children: elementActions)
      ]
      if !suggestions.isEmpty {
        children.append(
          UIMenu(
            title: "SmartType", options: .displayInline,
            children: suggestions.map { suggestion in
              UIAction(title: suggestion) { [weak self] _ in
                self?.applyLine(kind: suggestionKind, value: suggestion, range: contentRange)
              }
            }))
      }
      elementButton.menu = UIMenu(children: children)
      guard let selectedTextRange = textView.selectedTextRange else {
        elementButton.isHidden = true
        return
      }
      let caret = textView.caretRect(for: selectedTextRange.start)
      let target = CGFloat(PhoneWritingLayoutMetrics.standard.minimumHitTarget)
      let clearance = CGFloat(
        PhoneWritingLayoutMetrics.standard.screenplayInlineMenuClearance)
      elementButton.frame = CGRect(
        x: min(max(0, caret.maxX + clearance), textView.bounds.width - target),
        y: min(max(0, caret.midY - target / 2), textView.bounds.height - target),
        width: target, height: target)
      elementButton.isHidden = false
      textView.bringSubviewToFront(elementButton)
    }

    private func applyLine(kind: ScreenplayElementKind, value: String, range: NSRange) {
      guard
        let edit = ScreenplayInteraction.typingEdit(
          source: source, firstLine: firstLine, range: range,
          replacement: ScreenplayInteraction.formattedLine(value, as: kind))
      else { return }
      source = edit.source
      parent.client.enqueueEditorEdits(
        fileId: parent.file.file.fileId, edits: [edit.wireEdit])
      render(selection: NSRange(location: edit.cursor, length: 0))
      parent.onEditingActivity()
      updateElementMenu()
    }

    private func lineNumber(at utf16Offset: Int) -> UInt32 {
      let prefix = (source as NSString).substring(
        with: NSRange(location: 0, length: min(utf16Offset, source.utf16.count)))
      return firstLine + UInt32(prefix.reduce(into: 0) { if $1 == "\n" { $0 += 1 } })
    }

    private var screenplayElementKinds: [ScreenplayElementKind] {
      [.scene_heading, .action, .character, .parenthetical, .dialogue, .transition, .shot]
    }

    private func elementTitle(_ kind: ScreenplayElementKind) -> String {
      switch kind {
      case .scene_heading: "Scene heading"
      case .action: "Action"
      case .character: "Character"
      case .parenthetical: "Parenthetical"
      case .dialogue: "Dialogue"
      case .transition: "Transition"
      case .shot: "Shot"
      }
    }

    private func centerPage(in scrollView: UIScrollView) {
      let scaled = CGSize(
        width: pageView.bounds.width * scrollView.zoomScale,
        height: pageView.bounds.height * scrollView.zoomScale)
      let horizontal = max(0, (CGFloat(parent.viewportSize.width) - scaled.width) / 2)
      let vertical = max(0, (CGFloat(parent.viewportSize.height) - scaled.height) / 2)
      scrollView.contentInset = UIEdgeInsets(
        top: vertical, left: horizontal, bottom: vertical, right: horizontal)
    }

    private func updateTextScrolling(_ scrollView: UIScrollView) {
      let fitted = abs(scrollView.zoomScale - scrollView.minimumZoomScale) < 0.000_1
      if textView.isScrollEnabled != fitted {
        textView.isScrollEnabled = fitted
      }
    }

    private func render(selection: NSRange) {
      applying = true
      textView.attributedText = PhoneScreenplayStyle.attributed(
        source, firstLine: firstLine, structure: parent.structure)
      textView.selectedRange = valid(selection)
      textView.typingAttributes = PhoneScreenplayStyle.typingAttributes
      textView.accessibilityLabel = "\(parent.accessibilityName), screenplay"
      applying = false
    }

    private func updateAccessibilityValue(scale: CGFloat) {
      textView.accessibilityValue = "\(Int((scale * 100).rounded())) percent"
    }

    private func valid(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else {
        return NSRange(location: 0, length: 0)
      }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }
  }
}

@MainActor
private enum PhoneScreenplayStyle {
  static var typingAttributes: [NSAttributedString.Key: Any] {
    [
      .font: PhoneWritingMetrics.screenplayFont(),
      .foregroundColor: UIColor(PhonePalette.ink),
      .paragraphStyle: baseParagraph(),
    ]
  }

  static func attributed(
    _ source: String, firstLine: UInt32, structure: WritingStructureResult
  ) -> NSAttributedString {
    let result = NSMutableAttributedString(string: source, attributes: typingAttributes)
    for element in structure.elements {
      guard
        let range = range(
          from: element.startLine, through: element.endLine,
          source: source, firstLine: firstLine)
      else { continue }
      style(element.kind, range: range, in: result)
    }
    for variants in structure.variants {
      guard
        let range = range(
          from: variants.line, through: variants.line,
          source: source, firstLine: firstLine)
      else { continue }
      result.addAttributes(
        [
          .underlineStyle: NSUnderlineStyle.single.union(.patternDot).rawValue,
          .underlineColor: UIColor(PhonePalette.accent),
        ], range: range)
    }
    return result
  }

  private static func style(
    _ kind: ScreenplayElementKind, range: NSRange, in text: NSMutableAttributedString
  ) {
    let paragraph = baseParagraph()
    let line = text.attributedSubstring(from: range).string
    let projection = ScreenplayInteraction.lineProjection(source: line, kind: kind)
    switch kind {
    case .scene_heading:
      text.addAttribute(.font, value: PhoneWritingMetrics.screenplayFont(bold: true), range: range)
    case .action:
      break
    case .shot:
      text.addAttribute(.font, value: PhoneWritingMetrics.screenplayFont(bold: true), range: range)
    case .character:
      paragraph.alignment = .center
      text.addAttribute(.font, value: PhoneWritingMetrics.screenplayFont(bold: true), range: range)
    case .parenthetical:
      paragraph.alignment = .center
      text.addAttribute(.foregroundColor, value: UIColor(PhonePalette.secondary), range: range)
    case .dialogue:
      paragraph.firstLineHeadIndent = 120
      paragraph.headIndent = 120
      paragraph.tailIndent = -120
    case .transition:
      paragraph.alignment = projection.openingTransition ? .left : .right
      text.addAttribute(.foregroundColor, value: UIColor(PhonePalette.secondary), range: range)
    }
    text.addAttribute(.paragraphStyle, value: paragraph, range: range)
    hideMarkers(projection.hiddenRanges, lineRange: range, in: text)
  }

  private static func hideMarkers(
    _ ranges: [NSRange], lineRange: NSRange, in text: NSMutableAttributedString
  ) {
    for marker in ranges {
      let absolute = NSRange(location: lineRange.location + marker.location, length: marker.length)
      guard NSMaxRange(absolute) <= NSMaxRange(lineRange) else { continue }
      text.addAttributes(
        [
          .font: UIFont.monospacedSystemFont(ofSize: 0.1, weight: .regular),
          .foregroundColor: UIColor(PhonePalette.face),
        ], range: absolute)
    }
  }

  private static func baseParagraph() -> NSMutableParagraphStyle {
    let style = NSMutableParagraphStyle()
    style.minimumLineHeight = PaneLayoutContentMetrics.screenplayLineHeight
    style.maximumLineHeight = PaneLayoutContentMetrics.screenplayLineHeight
    style.paragraphSpacing = 8
    return style
  }

  private static func range(
    from start: UInt32, through end: UInt32, source: String, firstLine: UInt32
  ) -> NSRange? {
    guard start >= firstLine, end >= start else { return nil }
    let lines = source.components(separatedBy: "\n")
    let localStart = Int(start - firstLine)
    let localEnd = Int(end - firstLine)
    guard localStart < lines.count else { return nil }
    let clampedEnd = min(localEnd, lines.count - 1)
    let location = lines.prefix(localStart).reduce(0) { $0 + $1.utf16.count + 1 }
    let length = lines[localStart...clampedEnd].reduce(0) { $0 + $1.utf16.count + 1 } - 1
    return NSRange(location: location, length: max(0, length))
  }
}
