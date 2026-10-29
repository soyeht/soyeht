import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

@MainActor enum EditorTextMetrics {
  static let lineHeight: CGFloat = PaneLayoutContentMetrics.editorLineHeight
  static let font = MacTheme.monoNSFont(PaneLayoutContentMetrics.editorFontPointSize)
  static let characterWidth = ("0" as NSString).size(withAttributes: [.font: font]).width

  static func attributes(wraps: Bool) -> [NSAttributedString.Key: Any] {
    [
      .font: font,
      .foregroundColor: NSColor(MacTheme.ink),
      .paragraphStyle: paragraphStyle(wraps: wraps),
    ]
  }

  static func paragraphStyle(wraps: Bool) -> NSParagraphStyle {
    EditorTextLayout.paragraphStyle(
      lineHeight: lineHeight, characterWidth: characterWidth, wraps: wraps)
  }
}

struct EditorNativeViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  var view: EditorFileView
  @Binding var selectedPosition: TextPosition?
  let navigationTarget: EditorNavigationTarget?
  let diffMarkers: [UInt32: Set<EditorDiffMarker>]
  let agentReviewLines: Set<UInt32>
  let agentCursors: [EditorAgentCursorEvent]
  let vimEnabled: Bool
  @Binding var vimMode: EditorVimMode
  @Binding var vimCommandLine: String?
  let onDefinition: (TextPosition) -> Void
  let onReferences: (TextPosition) -> Void
  var find: EditorFindController? = nil
  var softWrap = false
  @Environment(\.askToClosePane) private var askToClosePane
  var onToggleSoftWrap: (() -> Void)? = nil

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeNSView(context: Context) -> EditorViewportHostView {
    context.coordinator.makeHostView()
  }

  func updateNSView(_ hostView: EditorViewportHostView, context: Context) {
    context.coordinator.parent = self
    hostView.layoutSubtreeIfNeeded()
    context.coordinator.update(view)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    private static let lineHeight = EditorTextMetrics.lineHeight
    var parent: EditorNativeViewport
    /// TextKit 1 on purpose: a plain text view starts on TextKit 2, which lays out every line
    /// of the page before the first draw, and the editor measurements run on TextKit 1.
    private let textView = EditorTextView(usingTextLayoutManager: false)
    private weak var scrollView: NSScrollView?
    private var gutter: EditorLineGutter?
    private var renderedIdentity: String?
    private var appliedNavigationId: String?
    private var viewportStart: UInt32 = 0
    /// The first line of the loaded page the text view holds (`EditorDisplayWindow`), and the
    /// page it was placed for.
    private var displayTop: UInt32?
    private var displayedFileId: String?
    private var shownViewport: FileReadResult?
    /// What the text on screen was rendered from: the file, revision, diagnostics and theme,
    /// and the last line of the page. Only lines above the window can then be missing.
    private var renderedContent: String?
    private var renderedTo: UInt32 = 0
    /// The first line of the selections on screen, so the window keeps them.
    private var selectionFloorLine: UInt32?
    private var wholePageRequested = false
    private var updating = false
    private var language = EditorLanguage.plain_text
    private var vim = EditorVimMachine()
    private var wasVimEnabled = false
    private var requestedViewportStart: UInt32?
    /// True from an undo or redo request until its text is on screen; edits
    /// wait so they are never computed against text the engine replaced.
    private var replayingHistory = false
    private var replayRendersAfterRevision: String?
    private var documentHeight: CGFloat = 0
    /// Rows of the loaded page; one per line unless soft wrap is on.
    private var wrapLayout = EditorWrapLayout(text: "", columns: nil)
    private var selectionPublishScheduled = false
    private var findHighlightKey = ""
    private var editorFocusRequest = 0

    init(_ parent: EditorNativeViewport) {
      self.parent = parent
    }

    #if SOYEHT_PERFORMANCE_METER
      var performanceTextView: EditorTextView { textView }
      var onPerformanceViewportRequested: ((UInt32) -> Void)?
      var isPerformanceLoadingViewport = false
    #endif

    func makeHostView() -> EditorViewportHostView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.hasHorizontalScroller = true
      scroll.autohidesScrollers = true
      scroll.borderType = .noBorder
      scroll.documentView = textView
      scroll.contentView.postsBoundsChangedNotifications = true
      scroll.contentView.postsFrameChangedNotifications = true
      NotificationCenter.default.addObserver(
        self, selector: #selector(visibleBoundsChanged(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      NotificationCenter.default.addObserver(
        self, selector: #selector(visibleFrameChanged(_:)),
        name: NSView.frameDidChangeNotification, object: scroll.contentView)
      configureTextView()
      // A separate clipped gutter keeps the scroll view from covering sibling SwiftUI content.
      let gutter = EditorLineGutter(scrollView: scroll, textView: textView)
      self.gutter = gutter
      scrollView = scroll
      let host = EditorViewportHostView(scrollView: scroll, gutter: gutter)
      DispatchQueue.main.async { [weak self] in
        guard let self else { return }
        self.renderedIdentity = nil
        self.renderedContent = nil
        self.update(self.parent.view)
      }
      return host
    }

    func update(_ view: EditorFileView) {
      guard let page = view.viewport else { return }
      applyTheme()
      let previousViewportStart = viewportStart
      language = view.file.language
      textView.inputDelegate = self
      textView.onToggleSoftWrap = parent.onToggleSoftWrap
      textView.vimEnabled = parent.vimEnabled
      if wasVimEnabled, !parent.vimEnabled { vim.reset() }
      wasVimEnabled = parent.vimEnabled
      publishVimState()
      applyWrap()
      updating = true
      defer { updating = false }
      let content =
        "\(page.file.fileId):\(page.file.revision):\(diagnosticIdentity(view.diagnostics)):\(SoyehtThemeStore.shared.activeTheme.rawValue)"
      let viewport = placeWindow(page, content: content, diagnostics: view.diagnostics)
      viewportStart = viewport.fromLine
      if requestedViewportStart == page.fromLine { requestedViewportStart = nil }
      gutter?.update(
        startLine: viewport.fromLine, markers: parent.diffMarkers,
        agentReviewLines: parent.agentReviewLines,
        agentCursors: parent.agentCursors, selectedLine: parent.selectedPosition?.line,
        lineCount: viewport.file.lineCount)
      updateVirtualGeometry(viewport)
      let identity = "\(content):\(viewport.fromLine):\(viewport.toLine)"
      if renderedIdentity != identity {
        let selection = textView.selectedRanges
        let carried = EditorDisplayWindow.carried(
          selection.map(\.rangeValue), text: textView.string, from: previousViewportStart,
          into: viewport)
        guard let storage = textView.textStorage else { return }
        storage.setAttributedString(attributed(viewport, diagnostics: view.diagnostics))
        renderedIdentity = identity
        renderedContent = content
        renderedTo = viewport.toLine
        measureWrap()
        updateVirtualGeometry(viewport)
        textView.selectedRanges =
          carried.map { $0.map(NSValue.init(range:)) }
          ?? selections(
            afterLoading: viewport, previousStart: previousViewportStart, previous: selection)
        textView.needsDisplay = true
        gutter?.needsDisplay = true
        // The text under the caret changed, so the status bar must re-read it.
        selectionChanged()
      }
      applyNavigation(viewport)
      applyFind(viewport)
      if let before = replayRendersAfterRevision, viewport.file.revision != before {
        replayRendersAfterRevision = nil
        replayingHistory = false
      }
      requestViewportIfNeeded()
      if windowTop(page) != displayTop, !textView.hasMarkedText() {
        DispatchQueue.main.async { [weak self] in self?.moveWindowIfNeeded() }
      }
    }

    /// Highlights the engine's matches on the loaded page and hands focus
    /// back to the text when the find bar closes.
    private func applyFind(_ viewport: FileReadResult) {
      let find = parent.find
      if let request = find?.editorFocusRequest, request != editorFocusRequest {
        editorFocusRequest = request
        textView.window?.makeFirstResponder(textView)
      }
      // Ranges from another revision would mark the wrong text; the bar
      // re-counts after every change and the highlights follow.
      let result =
        find?.isPresented == true && find?.result?.revision == viewport.file.revision
        ? find?.result : nil
      let visible = result?.visible ?? []
      let key =
        visible.isEmpty
        ? ""
        : "\(renderedIdentity ?? ""):\(result?.revision ?? ""):\(visible.count):\(visible.first.map { "\($0.start.line):\($0.start.column)" } ?? "")"
      guard key != findHighlightKey, let storage = textView.textStorage else { return }
      findHighlightKey = key
      storage.beginEditing()
      storage.removeAttribute(.backgroundColor, range: NSRange(location: 0, length: storage.length))
      let color = NSColor(MacTheme.warning).withAlphaComponent(0.28)
      for range in visible {
        let target = EditorNavigationTarget(
          id: "", fileId: viewport.file.fileId, position: range.start, selectionEnd: range.end)
        guard let local = EditorViewportPaging.landing(target, in: viewport), local.length > 0,
          NSMaxRange(local) <= storage.length
        else { continue }
        storage.addAttribute(.backgroundColor, value: color, range: local)
      }
      storage.endEditing()
    }

    func presentFind(replace: Bool) {
      let range = textView.selectedRange()
      let text = range.length > 0 ? (textView.string as NSString).substring(with: range) : nil
      parent.find?.present(replace: replace, selection: selectedTextRange(), selectedText: text)
    }

    func findNext(backward: Bool) {
      guard let find = parent.find else { return }
      guard !find.query.isEmpty else { return presentFind(replace: false) }
      if backward {
        find.previous(before: selectedTextRange())
      } else {
        find.next(after: selectedTextRange())
      }
    }

    /// Escape closes the find bar first; returns false when it was closed.
    func closeFind() -> Bool {
      guard let find = parent.find, find.isPresented else { return false }
      find.close()
      return true
    }

    private func selectedTextRange() -> SoyehtProtocol.TextRange? {
      let range = textView.selectedRange()
      guard let start = position(at: range.location),
        let end = position(at: NSMaxRange(range))
      else { return nil }
      return SoyehtProtocol.TextRange(start: start, end: end)
    }

    private func applyTheme() {
      scrollView?.backgroundColor = NSColor(MacTheme.face)
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
    }

    @objc private func visibleBoundsChanged(_ notification: Notification) {
      moveWindowIfNeeded()
      requestViewportIfNeeded()
    }

    /// Moves the window up when the view scrolled back to its top, or a line that has to be held
    /// lies above it. Text being composed waits for the end of the composition.
    private func moveWindowIfNeeded() {
      guard !updating, let page = parent.view.viewport, !textView.hasMarkedText(),
        windowTop(page) != displayTop
      else { return }
      update(parent.view)
    }

    /// Select All covers the whole loaded page, not only the window.
    func holdWholePage() {
      guard !updating, !textView.hasMarkedText(), let page = parent.view.viewport,
        displayTop != page.fromLine
      else { return }
      wholePageRequested = true
      update(parent.view)
      wholePageRequested = false
    }

    @objc private func visibleFrameChanged(_ notification: Notification) {
      guard let scrollView else { return }
      if applyWrap(), let viewport = shownViewport {
        updateVirtualGeometry(viewport)
        return
      }
      EditorTextLayout.fit(
        textView, visible: scrollView.contentSize, documentHeight: documentHeight)
    }

    /// Follows the soft wrap choice and the visible width; true when the cells per row
    /// changed, which re-wraps the page.
    @discardableResult private func applyWrap() -> Bool {
      guard let scrollView else { return false }
      let columns =
        parent.softWrap
        ? EditorTextLayout.wrapColumns(
          textView, visibleWidth: scrollView.contentSize.width,
          characterWidth: EditorTextMetrics.characterWidth)
        : nil
      guard columns != wrapLayout.columns else { return false }
      scrollView.hasHorizontalScroller = columns == nil
      EditorTextLayout.configure(
        textView, wrapColumns: columns, characterWidth: EditorTextMetrics.characterWidth)
      textView.typingAttributes = baseAttributes(wraps: columns != nil)
      if let storage = textView.textStorage, storage.length > 0 {
        storage.addAttribute(
          .paragraphStyle, value: EditorTextMetrics.paragraphStyle(wraps: columns != nil),
          range: NSRange(location: 0, length: storage.length))
      }
      wrapLayout = EditorWrapLayout(text: "", columns: columns)
      measureWrap()
      return true
    }

    /// Re-counts the rows of the page text on screen; it never reads beyond the page.
    private func measureWrap() {
      wrapLayout = EditorWrapLayout(text: textView.string, columns: wrapLayout.columns)
      gutter?.setLayout(wrapLayout)
    }

    private func updateVirtualGeometry(_ viewport: FileReadResult) {
      guard let scrollView else { return }
      let top =
        PaneLayoutContentMetrics.editorTextInset + CGFloat(viewport.fromLine) * Self.lineHeight
      textView.textContainerInset = NSSize(
        width: PaneLayoutContentMetrics.editorTextInset, height: top)
      documentHeight =
        2 * PaneLayoutContentMetrics.editorTextInset
        + CGFloat(wrapLayout.documentRows(totalLines: viewport.file.lineCount)) * Self.lineHeight
      EditorTextLayout.fit(
        textView, visible: scrollView.contentSize, documentHeight: documentHeight)
    }

    /// The page as the text view holds it. The text is made to match: kept, extended upwards
    /// with the lines the window now covers, or left for the render that follows.
    private func placeWindow(
      _ page: FileReadResult, content: String, diagnostics: [FileDiagnostic]
    ) -> FileReadResult {
      if displayedFileId != page.file.fileId {
        displayedFileId = page.file.fileId
        displayTop = nil
        renderedContent = nil
      }
      let desired = windowTop(page)
      let change = EditorDisplayWindow.change(
        desired: desired, current: displayTop,
        holdsPage: renderedContent == content && renderedTo == page.toLine,
        composing: textView.hasMarkedText())
      var top = displayTop ?? desired
      switch change {
      case .keep: break
      case .replace(let replaced): top = replaced
      case .extendUp(let extended):
        extend(page, to: extended, diagnostics: diagnostics)
        top = extended
        renderedIdentity = "\(content):\(top):\(page.toLine)"
      }
      displayTop = top
      let shown = EditorDisplayWindow.viewport(page, top: top)
      shownViewport = shown
      return shown
    }

    private func extend(_ page: FileReadResult, to top: UInt32, diagnostics: [FileDiagnostic]) {
      guard let current = displayTop else { return }
      let head = EditorDisplayWindow.lines(page, from: top, to: current)
      let text = NSMutableAttributedString(
        attributedString: attributed(head, diagnostics: diagnostics))
      text.append(NSAttributedString(string: "\n", attributes: baseAttributes(wraps: false)))
      EditorTextLayout.prepend(text, to: textView)
      measureWrap()
    }

    /// The first line of `page` the text view holds. Soft wrap holds the whole page: the rows
    /// of lines above the window are estimates, so a window would move the text when it moves.
    /// Selections and a navigation target on the page stay in the window.
    private func windowTop(_ page: FileReadResult) -> UInt32 {
      guard wrapLayout.columns == nil else { return page.fromLine }
      var required = wholePageRequested ? page.fromLine : selectionFloorLine
      if let target = parent.navigationTarget, target.fileId == page.file.fileId,
        target.id != appliedNavigationId
      {
        // The target lands with the window's lead above it, so that putting it in view does
        // not call for another move.
        let line = target.position.line
        if line >= page.fromLine, line < page.toLine {
          let held = max(
            page.fromLine, line > EditorDisplayWindow.lead ? line - EditorDisplayWindow.lead : 0)
          required = min(required ?? held, held)
        }
      }
      return EditorDisplayWindow.top(
        pageFrom: page.fromLine, pageTo: page.toLine, current: displayTop,
        visibleFrom: firstVisibleLine(), required: required)
    }

    private func firstVisibleLine() -> UInt32 {
      guard let scrollView else { return 0 }
      let top = max(
        0, scrollView.contentView.bounds.minY - PaneLayoutContentMetrics.editorTextInset)
      return UInt32(clamping: Int(top / Self.lineHeight))
    }

    private func requestViewportIfNeeded() {
      #if SOYEHT_PERFORMANCE_METER
        if isPerformanceLoadingViewport { return }
      #endif
      guard let scrollView, let viewport = parent.view.viewport,
        requestedViewportStart == nil
      else { return }
      let visibleTop = max(
        0, scrollView.contentView.bounds.minY - PaneLayoutContentMetrics.editorTextInset)
      let visibleFrom = wrapLayout.logicalLine(
        atDocumentRow: Int(visibleTop / Self.lineHeight),
        fromLine: shownViewport?.fromLine ?? viewport.fromLine)
      let visibleLines = UInt32(
        clamping: Int(ceil(scrollView.contentSize.height / Self.lineHeight)) + 2)
      guard
        let start = EditorViewportPaging.requestedStart(
          visibleFrom: visibleFrom, visibleLines: visibleLines,
          loadedFrom: viewport.fromLine, loadedTo: viewport.toLine,
          totalLines: viewport.file.lineCount,
          askedLines: parent.view.viewportRequestedLines)
      else { return }
      requestedViewportStart = start
      let fileId = viewport.file.fileId
      #if SOYEHT_PERFORMANCE_METER
        if let onPerformanceViewportRequested {
          onPerformanceViewportRequested(start)
          return
        }
      #endif
      Task { @MainActor in
        do {
          try await parent.client.loadEditorViewport(fileId: fileId, fromLine: start)
        } catch {
          parent.client.problem = error.localizedDescription
        }
        requestedViewportStart = nil
      }
    }

    func insert(_ inserted: String, ranges: [NSRange]) -> Bool {
      if replayingHistory { return true }
      guard
        let plan = EditorEditing.insertionPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          inserted: inserted, language: language)
      else { return false }
      apply(plan)
      return true
    }

    func delete(_ ranges: [NSRange], backward: Bool) -> Bool {
      if replayingHistory { return true }
      guard
        let plan = EditorEditing.deletionPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          backward: backward, language: language)
      else { return false }
      apply(plan)
      return true
    }

    func replaceVerbatim(_ ranges: [NSRange], with strings: [String]) -> Bool {
      guard !replayingHistory,
        let plan = EditorEditing.verbatimPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          strings: strings)
      else { return false }
      apply(plan)
      return true
    }

    func replayHistory(_ direction: EditorHistoryDirection) {
      guard !replayingHistory else { return }
      replayingHistory = true
      let fileId = parent.view.file.fileId
      let before = parent.view.file.revision
      Task { @MainActor in
        var applied = false
        await report {
          applied = try await self.parent.client.replayEditorHistory(fileId, direction: direction)
            .applied
        }
        if applied, parent.view.viewport?.file.revision == before {
          replayRendersAfterRevision = before
        } else {
          replayingHistory = false
        }
      }
    }

    func toggleComment(_ ranges: [NSRange]) {
      if replayingHistory { return }
      guard
        let plan = EditorEditing.toggleCommentPlan(
          text: textView.string, viewportStartLine: viewportStart, ranges: ranges,
          language: language)
      else { return }
      apply(plan)
    }

    func selectNextOccurrence(_ ranges: [NSRange]) {
      var selections = ranges
      if selections.first?.length == 0, let offset = selections.first?.location,
        let word = EditorEditing.wordRange(text: textView.string, at: offset)
      {
        selections = [word]
      } else if let next = EditorEditing.nextOccurrence(text: textView.string, ranges: selections) {
        selections.append(next)
      }
      textView.selectedRanges = selections.map(NSValue.init(range:))
      selectionChanged()
    }

    func addCursor(at offset: Int) {
      var selections = textView.selectedRanges.map(\.rangeValue)
      let added = NSRange(location: offset, length: 0)
      guard !selections.contains(added) else { return }
      selections.append(added)
      textView.selectedRanges = selections.sorted { $0.location < $1.location }.map(
        NSValue.init(range:))
      selectionChanged()
    }

    func goToDefinition(at offset: Int) {
      guard let position = position(at: offset) else { return }
      parent.onDefinition(position)
    }

    func findReferences(at offset: Int) {
      guard let position = position(at: offset) else { return }
      parent.onReferences(position)
    }

    func handleVim(_ key: EditorVimKey, ranges: [NSRange]) -> Bool {
      guard parent.vimEnabled else { return false }
      let result = vim.handle(
        key, text: textView.string, viewportStartLine: viewportStart,
        selections: ranges, language: language)
      if let plan = result.plan { apply(plan) }
      if let selections = result.selections {
        textView.selectedRanges = selections.map(NSValue.init(range:))
        selectionChanged()
      }
      if let effect = result.effect { perform(effect) }
      publishVimState()
      return result.handled
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      selectionChanged()
    }

    private func configureTextView() {
      textView.delegate = self
      textView.inputDelegate = self
      textView.isRichText = false
      textView.importsGraphics = false
      // Undo belongs to the engine-owned history; AppKit's local stack would
      // rewind text the engine never sees.
      textView.allowsUndo = false
      // AppKit's find panel only sees the loaded page; Cmd+F opens the engine-backed bar.
      textView.usesFindPanel = false
      textView.usesFindBar = false
      textView.isAutomaticQuoteSubstitutionEnabled = false
      textView.isAutomaticDashSubstitutionEnabled = false
      textView.isAutomaticTextReplacementEnabled = false
      textView.isAutomaticSpellingCorrectionEnabled = false
      textView.isContinuousSpellCheckingEnabled = false
      textView.drawsBackground = true
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
      textView.textContainerInset = NSSize(width: 10, height: 10)
      EditorTextLayout.configure(textView)
      textView.typingAttributes = baseAttributes(wraps: false)
    }

    private func apply(_ plan: EditorInputPlan) {
      let changes = plan.replacements.filter { $0.range.length > 0 || !$0.text.isEmpty }
      if !changes.isEmpty {
        let storage = textView.textStorage
        storage?.beginEditing()
        for replacement in changes.sorted(by: { $0.range.location > $1.range.location }) {
          storage?.replaceCharacters(in: replacement.range, with: replacement.text)
        }
        storage?.endEditing()
        textView.didChangeText()
      }
      textView.selectedRanges = plan.selections.map(NSValue.init(range:))
      selectionChanged()
      measureWrap()
      if !plan.edits.isEmpty {
        parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: plan.edits)
      }
    }

    private func attributed(
      _ viewport: FileReadResult, diagnostics: [FileDiagnostic]
    ) -> NSAttributedString {
      let text = EditorEditing.viewportText(viewport.lines)
      let result = NSMutableAttributedString(
        string: text, attributes: baseAttributes(wraps: wrapLayout.columns != nil))
      var offset = 0
      for line in viewport.lines {
        let length = line.text.utf16.count
        for span in line.highlights {
          let start = min(Int(span.startColumn), length)
          let end = min(max(Int(span.endColumn), start), length)
          result.addAttribute(
            .foregroundColor, value: highlightColor(span.kind),
            range: NSRange(location: offset + start, length: end - start))
        }
        offset += length + 1
      }
      for diagnostic in diagnostics {
        guard let range = EditorDiagnostics.localRange(diagnostic, in: viewport) else { continue }
        result.addAttributes(
          [
            .underlineStyle: NSUnderlineStyle.single.rawValue,
            .underlineColor: diagnosticColor(diagnostic.severity),
            .toolTip: "\(diagnostic.source) · \(diagnostic.message)",
          ], range: range)
      }
      return result
    }

    private func diagnosticIdentity(_ diagnostics: [FileDiagnostic]) -> String {
      diagnostics.map {
        "\($0.diagnosticId):\($0.range.start.line):\($0.range.start.column):\($0.range.end.line):\($0.range.end.column):\($0.severity.rawValue):\($0.message)"
      }.joined(separator: "|")
    }

    private func diagnosticColor(_ severity: DiagnosticSeverity) -> NSColor {
      switch severity {
      case .error: NSColor(MacTheme.danger)
      case .warning: NSColor(MacTheme.warning)
      case .information: NSColor(MacTheme.accent)
      case .hint: NSColor(MacTheme.muted)
      }
    }

    private func baseAttributes(wraps: Bool) -> [NSAttributedString.Key: Any] {
      EditorTextMetrics.attributes(wraps: wraps)
    }

    private func highlightColor(_ kind: HighlightKind) -> NSColor {
      switch kind.role {
      case .accent: NSColor(MacTheme.accent)
      case .syntaxType: NSColor(MacTheme.syntaxType)
      case .warning: NSColor(MacTheme.warning)
      case .muted: NSColor(MacTheme.muted)
      case .success: NSColor(MacTheme.success)
      case .ink: NSColor(MacTheme.ink)
      }
    }

    private func validSelections(_ values: [NSValue], length: Int) -> [NSValue] {
      let valid = values.filter { NSMaxRange($0.rangeValue) <= length }
      return valid.isEmpty ? [NSValue(range: NSRange(location: 0, length: 0))] : valid
    }

    private func selections(
      afterLoading viewport: FileReadResult, previousStart: UInt32, previous: [NSValue]
    ) -> [NSValue] {
      guard previousStart != viewport.fromLine else {
        return validSelections(previous, length: textView.string.utf16.count)
      }
      guard let position = parent.selectedPosition,
        let offset = EditorViewportPaging.localOffset(position, in: viewport)
      else { return [NSValue(range: NSRange(location: 0, length: 0))] }
      return [NSValue(range: NSRange(location: offset, length: 0))]
    }

    /// Lands an undo, redo, go-to or find target: the caret (or the match
    /// selection) clamped to the loaded text, scrolled into view.
    private func applyNavigation(_ viewport: FileReadResult) {
      guard let target = parent.navigationTarget, target.fileId == viewport.file.fileId,
        target.id != appliedNavigationId,
        let range = EditorViewportPaging.landing(target, in: viewport),
        NSMaxRange(range) <= textView.string.utf16.count
      else { return }
      appliedNavigationId = target.id
      textView.setSelectedRange(range)
      textView.scrollRangeToVisible(range)
      selectionChanged()
    }

    /// Publishes the caret to the status bar and gutter on the next turn of
    /// the run loop. A binding written inside a SwiftUI view update can be
    /// dropped, which left the status bar on the pre-undo position.
    private func selectionChanged() {
      guard !selectionPublishScheduled else { return }
      selectionPublishScheduled = true
      DispatchQueue.main.async { [weak self] in
        guard let self else { return }
        self.selectionPublishScheduled = false
        let position = self.position(at: self.textView.selectedRange().location)
        self.selectionFloorLine = self.floorLine(of: self.textView.selectedRanges, first: position)
        self.parent.selectedPosition = position
        self.gutter?.selectedLine = position?.line
        self.gutter?.needsDisplay = true
      }
    }

    /// The line of the first selection end; with several selections, the topmost.
    private func floorLine(of ranges: [NSValue], first: TextPosition?) -> UInt32? {
      guard ranges.count > 1 else { return first?.line }
      return ranges.compactMap { position(at: $0.rangeValue.location)?.line }.min()
    }

    private func publishVimState() {
      parent.vimMode = vim.mode
      parent.vimCommandLine = vim.commandLine
    }

    private func perform(_ effect: EditorVimEffect) {
      let fileId = parent.view.file.fileId
      switch effect {
      case .close:
        parent.askToClosePane(parent.view.file.paneId)
      case .save:
        Task { @MainActor in await report { try await self.parent.client.saveEditorFile(fileId) } }
      case .reload:
        Task { @MainActor in await report { try await self.parent.client.reloadEditorFile(fileId) }
        }
      case .undo:
        replayHistory(.undo)
      case .substitute(let pattern, let replacement, let everyMatch):
        Task { @MainActor in
          await report {
            try await self.parent.client.reloadEditorFile(fileId)
            guard let revision = self.parent.client.editorFiles[fileId]?.file.revision else {
              throw ClientFailure.invalidSnapshot
            }
            _ = try await self.parent.client.substituteEditorFile(
              fileId: fileId, baseRevision: revision, pattern: pattern,
              replacement: replacement, everyMatch: everyMatch)
          }
        }
      }
    }

    private func report(_ operation: () async throws -> Void) async {
      do { try await operation() } catch { parent.client.problem = error.localizedDescription }
    }

    private func position(at offset: Int) -> TextPosition? {
      EditorEditing.textPosition(
        text: textView.string, viewportStartLine: viewportStart, utf16Offset: offset)
    }
  }
}

@MainActor final class EditorViewportHostView: NSView {
  let scrollView: NSScrollView
  let gutter: EditorLineGutter

  init(scrollView: NSScrollView, gutter: EditorLineGutter) {
    self.scrollView = scrollView
    self.gutter = gutter
    super.init(frame: .zero)
    wantsLayer = true
    layer?.masksToBounds = true
    scrollView.translatesAutoresizingMaskIntoConstraints = false
    gutter.translatesAutoresizingMaskIntoConstraints = false
    addSubview(gutter)
    addSubview(scrollView)
    NSLayoutConstraint.activate([
      gutter.leadingAnchor.constraint(equalTo: leadingAnchor),
      gutter.topAnchor.constraint(equalTo: topAnchor),
      gutter.bottomAnchor.constraint(equalTo: bottomAnchor),
      gutter.widthConstraint,
      scrollView.leadingAnchor.constraint(equalTo: gutter.trailingAnchor),
      scrollView.trailingAnchor.constraint(equalTo: trailingAnchor),
      scrollView.topAnchor.constraint(equalTo: topAnchor),
      scrollView.bottomAnchor.constraint(equalTo: bottomAnchor),
    ])
  }

  required init?(coder: NSCoder) {
    nil
  }
}
