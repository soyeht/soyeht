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
    private weak var outerScrollView: UIScrollView?
    private var text = WritingViewportText()
    private var renderedIdentity: String?
    private var applying = false

    init(_ parent: PhoneWritingScreenplaySurface) {
      self.parent = parent
    }

    func makeScrollView() -> UIScrollView {
      let scrollView = UIScrollView()
      outerScrollView = scrollView
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
      let chromePlan = PhoneWritingMetrics.chromePolicy.plan(kind: .screenplay)
      elementButton.frame.size = CGSize(
        width: chromePlan.minimumActionTarget,
        height: chromePlan.minimumActionTarget)
      elementButton.showsMenuAsPrimaryAction = true
      elementButton.isAccessibilityElement = true
      elementButton.accessibilityTraits = .button
      elementButton.accessibilityLabel = "Screenplay element and SmartType"
      elementButton.accessibilityIdentifier =
        "phone-writing-screenplay-element-\(parent.identity.paneId)"
      elementButton.isHidden = true

      pageView.addSubview(textView)
      scrollView.addSubview(pageView)
      scrollView.addSubview(elementButton)
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
      text.firstLine = viewport.fromLine
      let identity = [
        viewport.file.revision,
        String(viewport.fromLine),
        String(viewport.toLine),
        parent.writing.document.sidecarRevision,
        String(describing: SoyehtThemeStore.shared.activeTheme),
      ].joined(separator: ":")
      if identity != renderedIdentity {
        text.source = EditorEditing.viewportText(viewport.lines)
        renderedIdentity = identity
        render(selection: text.clamped(textView.selectedRange))
      }
      updateElementMenu()
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? {
      pageView
    }

    func scrollViewDidZoom(_ scrollView: UIScrollView) {
      centerPage(in: scrollView)
      updateTextScrolling(scrollView)
      updateElementMenu()
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
      guard scrollView === textView || scrollView === outerScrollView else { return }
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
      replacementText: String
    ) -> Bool {
      guard !applying,
        let edit = ScreenplayInteraction.typingEdit(
          source: text.source, firstLine: text.firstLine, range: range, replacement: replacementText
        )
      else { return false }
      text.source = edit.source
      parent.client.enqueueEditorEdits(
        fileId: parent.file.file.fileId, edits: [edit.wireEdit])
      render(selection: NSRange(location: edit.cursor, length: 0))
      parent.onEditingActivity()
      updateElementMenu()
      return false
    }

    private func updateElementMenu() {
      guard let scrollView = outerScrollView else {
        elementButton.isHidden = true
        return
      }
      let selection = textView.selectedRange
      guard selection.location != NSNotFound, selection.length == 0,
        selection.location <= text.source.utf16.count
      else {
        elementButton.isHidden = true
        return
      }
      let contentRange = text.contentRange(atLocation: selection.location)
      let lineText = (text.source as NSString).substring(with: contentRange)
      guard
        PhoneWritingMetrics.chromePolicy.writingChrome.showsScreenplayElementMenu(
          lineText: lineText, selectionLength: selection.length)
      else {
        elementButton.isHidden = true
        return
      }
      let activeLine = text.line(utf16Offset: selection.location)
      let suggestionKind = ScreenplayInteraction.suggestionKind(
        at: activeLine, in: parent.structure)
      let suggestions = Array(
        ScreenplayInteraction.suggestions(
          for: suggestionKind, prefix: lineText, in: parent.structure
        ).prefix(5))
      let elementActions = WritingDisplayText.screenplayElementKinds.map { kind in
        UIAction(title: WritingDisplayText.screenplayElementName(kind)) { [weak self] _ in
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
      let caretInPage = textView.convert(caret, to: pageView)
      let chromePlan = PhoneWritingMetrics.chromePolicy.plan(kind: .screenplay)
      let policy = PhoneScreenplayElementOverlayPolicy(
        minimumTarget: chromePlan.minimumActionTarget,
        clearance: PhoneWritingLayoutMetrics.standard.screenplayInlineMenuClearance)
      let pageRectInScroll = pageView.convert(pageView.bounds, to: scrollView)
      let converted = policy.caretRectInScroll(
        caretRect: PaneLayoutRect(
          origin: PaneLayoutPoint(x: caretInPage.minX, y: caretInPage.minY),
          size: PaneLayoutSize(width: caretInPage.width, height: caretInPage.height)),
        pageOrigin: PaneLayoutPoint(x: pageRectInScroll.minX, y: pageRectInScroll.minY),
        zoomScale: scrollView.zoomScale)
      let visible = PaneLayoutRect(
        origin: PaneLayoutPoint(x: scrollView.bounds.minX, y: scrollView.bounds.minY),
        size: PaneLayoutSize(width: scrollView.bounds.width, height: scrollView.bounds.height))
      guard let button = policy.buttonRect(for: converted, visibleRect: visible) else {
        elementButton.isHidden = true
        return
      }
      elementButton.frame = CGRect(
        x: button.origin.x, y: button.origin.y,
        width: button.size.width, height: button.size.height)
      elementButton.isHidden = false
      scrollView.bringSubviewToFront(elementButton)
    }

    private func applyLine(kind: ScreenplayElementKind, value: String, range: NSRange) {
      guard
        let edit = ScreenplayInteraction.typingEdit(
          source: text.source, firstLine: text.firstLine, range: range,
          replacement: ScreenplayInteraction.formattedLine(value, as: kind))
      else { return }
      text.source = edit.source
      parent.client.enqueueEditorEdits(
        fileId: parent.file.file.fileId, edits: [edit.wireEdit])
      render(selection: NSRange(location: edit.cursor, length: 0))
      parent.onEditingActivity()
      updateElementMenu()
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
        text, structure: parent.structure)
      textView.selectedRange = text.clamped(selection)
      textView.typingAttributes = PhoneScreenplayStyle.typingAttributes
      textView.accessibilityLabel = "\(parent.accessibilityName), screenplay"
      applying = false
    }

    private func updateAccessibilityValue(scale: CGFloat) {
      textView.accessibilityValue = "\(Int((scale * 100).rounded())) percent"
    }
  }
}

@MainActor
private enum PhoneScreenplayStyle {
  static var typingAttributes: [NSAttributedString.Key: Any] {
    [
      .font: PhoneWritingMetrics.screenplayFont(),
      .foregroundColor: UIColor(PhonePalette.ink),
      .paragraphStyle: ScreenplayLineStyle.basePageParagraph(),
    ]
  }

  static func attributed(
    _ text: WritingViewportText, structure: WritingStructureResult
  ) -> NSAttributedString {
    let result = NSMutableAttributedString(string: text.source, attributes: typingAttributes)
    let appearance = ScreenplayPageAppearance(
      boldFont: PhoneWritingMetrics.screenplayFont(bold: true),
      secondaryInk: UIColor(PhonePalette.secondary),
      hiddenMarkerFont: UIFont.monospacedSystemFont(
        ofSize: ScreenplayLineStyle.hiddenMarkerFontSize, weight: .regular),
      hiddenMarkerInk: UIColor(PhonePalette.face))
    for (kind, range) in text.elementRanges(in: structure) {
      ScreenplayLineStyle(kind: kind, line: result.attributedSubstring(from: range).string)
        .apply(to: result, range: range, appearance: appearance)
    }
    for range in text.variantRanges(in: structure) {
      result.addAttributes(
        [
          .underlineStyle: NSUnderlineStyle.single.union(.patternDot).rawValue,
          .underlineColor: UIColor(PhonePalette.accent),
        ], range: range)
    }
    return result
  }
}
