import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingPaneHeaderAction {
  let title: String
  let systemImage: String
  let help: String
  let accessibilityIdentifier: String
  let action: () -> Void
}

/// Shared writing chrome. Text adopts it in C3; screenplay and presentation follow in C4/C5.
struct WritingPaneHeader: View {
  let paneId: String
  let title: String
  let icon: String
  let metadata: String?
  let fullWidth: Bool?
  let headerDrag: PaneHeaderDragConfiguration?
  var auxiliaryAction: WritingPaneHeaderAction?
  let mutationDisabled: Bool
  let layoutActionsDisabled: Bool
  let onToggleFullWidth: (() -> Void)?
  let onSplit: (String, PaneSplitAxis) -> Void
  let onClose: (String) -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: icon).foregroundStyle(MacTheme.secondary)
      Text(title)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
      if let metadata {
        Text(metadata)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.muted)
          .lineLimit(1)
      }
      if let auxiliaryAction {
        Button(action: auxiliaryAction.action) {
          Label(auxiliaryAction.title, systemImage: auxiliaryAction.systemImage)
        }
        .buttonStyle(NeoPillButtonStyle())
        .help(auxiliaryAction.help)
        .accessibilityIdentifier(auxiliaryAction.accessibilityIdentifier)
      }
      if let fullWidth, let onToggleFullWidth {
        Button(action: onToggleFullWidth) {
          Image(
            systemName: fullWidth
              ? "arrow.down.right.and.arrow.up.left"
              : "arrow.up.left.and.arrow.down.right")
        }
        .help(fullWidth ? "Use reading width" : "Use full width")
        .accessibilityLabel(fullWidth ? "Use reading width" : "Use full width")
        .accessibilityIdentifier("writing-full-width-\(paneId)")
        .disabled(mutationDisabled)
      }
      Button {
        onSplit(paneId, .columns)
      } label: {
        Image(systemName: "rectangle.split.2x1")
      }
      .help("Split pane into columns")
      .accessibilityLabel("Split pane into columns")
      .accessibilityIdentifier("split-pane-columns-\(paneId)")
      .disabled(layoutActionsDisabled)
      Button {
        onSplit(paneId, .rows)
      } label: {
        Image(systemName: "rectangle.split.1x2")
      }
      .help("Split pane into rows")
      .accessibilityLabel("Split pane into rows")
      .accessibilityIdentifier("split-pane-rows-\(paneId)")
      .disabled(layoutActionsDisabled)
      Button {
        onClose(paneId)
      } label: {
        Image(systemName: "xmark")
      }
      .help("Close this pane")
      .accessibilityLabel("Close this pane")
      .accessibilityIdentifier("close-pane-\(paneId)")
      .disabled(layoutActionsDisabled)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.writingHeaderHeight)
    .background(MacTheme.well, in: Capsule())
    .paneHeaderDrag(headerDrag)
  }
}

struct WritingTextPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var selection: SoyehtProtocol.TextRange?
  @State private var activeLine: UInt32 = 0
  @State private var destinationKind: WritingFormatKind?
  @State private var destination = ""

  private var writing: WritingDocumentView? {
    client.writingDocuments.values.first { $0.document.file.paneId == pane.paneId }
  }
  private var file: EditorFileView? {
    guard let fileId = writing?.document.file.fileId else { return nil }
    return client.editorFiles[fileId]
  }
  private var writingLoadKey: String {
    writing?.loadIdentity ?? "missing"
  }

  var body: some View {
    GeometryReader { proxy in
      if let document = writing?.document,
        let plan = layoutPlan(size: proxy.size, document: document)
      {
        ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
          plannedSurface(plan: plan, document: document)
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDisabled(plan.overflowAxes.isEmpty)
        .frame(width: proxy.size.width, height: proxy.size.height)
      } else {
        VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
          writingHeader(document: nil)
          ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(PaneLayoutContentMetrics.paneCardInset)
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
      }
    }
    .task(id: writingLoadKey) { await loadIfNeeded() }
  }

  private func layoutPlan(size: CGSize, document: WritingDocument) -> WritingLayoutPlan? {
    let policy = WritingLayoutPolicy(
      floors: MacPaneLayoutMetrics.surfaceFloors,
      gaps: MacPaneLayoutMetrics.sizing.gaps)
    return try? policy.layout(
      WritingLayoutRequest(
        kind: .text,
        viewport: PaneLayoutSize(width: size.width, height: size.height),
        font: MacPaneLayoutMetrics.proseMetric(for: document.font),
        fullWidth: document.fullWidth,
        reviewVisible: file?.review != nil))
  }

  private func plannedSurface(
    plan: WritingLayoutPlan, document: WritingDocument
  ) -> some View {
    ZStack(alignment: .topLeading) {
      // Give the layout coordinate space its full planned size before positioning children.
      // Positioned children do not contribute their position to the ZStack's ideal size.
      Color.clear
        .frame(width: plan.contentSize.width, height: plan.contentSize.height)
        .allowsHitTesting(false)
      MacTheme.canvas
        .writingFrame(plan.contentRect)
      writingHeader(document: document)
        .writingFrame(plan.headerRect)
      if let toolbarRect = plan.toolbarRect {
        toolbar(document)
          .writingFrame(toolbarRect)
      }
      if let reviewRect = plan.reviewRect, let file {
        WritingTextReviewStrip(
          client: client, view: file, selectedPosition: selection?.start)
          .writingFrame(reviewRect)
      }
      if let file, file.viewport != nil,
        case .text(let textPlan) = plan.surface
      {
        WritingTextViewport(
          client: client, view: file, document: document,
          selection: $selection, activeLine: $activeLine
        )
        .writingFrame(textPlan.columnRect)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
        .accessibilityIdentifier("writing-text-column-\(pane.paneId)")
      } else if case .text(let textPlan) = plan.surface {
        ProgressView()
          .writingFrame(textPlan.columnRect)
      }
    }
    .frame(
      width: plan.contentSize.width, height: plan.contentSize.height,
      alignment: .topLeading)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
  }

  private func writingHeader(document: WritingDocument?) -> some View {
    WritingPaneHeader(
      paneId: pane.paneId, title: pane.name, icon: "doc.richtext",
      metadata: document == nil ? nil : wordCountLabel,
      fullWidth: document?.fullWidth,
      headerDrag: headerDrag,
      auxiliaryAction: nil,
      mutationDisabled: !client.connected,
      layoutActionsDisabled: !client.connected || layoutActionsDisabled,
      onToggleFullWidth: document.map { document in
        { setFullWidth(!document.fullWidth) }
      },
      onSplit: onSplit, onClose: onClose)
  }

  private func scrollAxes(_ plan: WritingLayoutPlan) -> Axis.Set {
    var axes: Axis.Set = []
    if plan.overflowAxes.contains(.horizontal) { axes.insert(.horizontal) }
    if plan.overflowAxes.contains(.vertical) { axes.insert(.vertical) }
    return axes
  }

  private func toolbar(_ document: WritingDocument) -> some View {
    ScrollView(.horizontal) {
      HStack(spacing: MacTheme.Space.x8) {
        Menu {
          fontButton("Newsreader", .reading_serif, document: document)
          fontButton("Classic serif", .classic_serif, document: document)
          fontButton("Sans", .sans, document: document)
        } label: {
          Label(fontLabel(document.font), systemImage: "textformat")
        }
        .menuStyle(.borderlessButton).frame(width: 174)

        Divider().frame(height: 20)
        Menu {
          formatButton("Heading 1", .heading1)
          formatButton("Heading 2", .heading2)
          formatButton("Heading 3", .heading3)
        } label: {
          Label("Heading", systemImage: "textformat.size")
        }
        .menuStyle(.borderlessButton).frame(width: 130)
        Divider().frame(height: 20)

        formatIcon("bold", "Bold", .bold)
        formatIcon("italic", "Italic", .italic)
        formatIcon("list.bullet", "Bulleted list", .bullet_list)
        formatIcon("list.number", "Numbered list", .numbered_list)
        formatIcon("quote.opening", "Quote", .quote)
        destinationIcon("link", "Link", .link)
        destinationIcon("photo", "Image", .image)
        Button {
          setFocus(!document.focusMode)
        } label: {
          Label("Focus", systemImage: "scope")
            .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
            .foregroundStyle(document.focusMode ? MacTheme.accent : MacTheme.secondary)
            .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
            .background(document.focusMode ? MacTheme.selection : Color.clear, in: Capsule())
        }
        .help("Emphasize the active line")
      }
      .fixedSize(horizontal: true, vertical: false)
    }
    .scrollIndicators(.hidden)
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: PaneLayoutContentMetrics.textToolbarHeight)
    .background(MacTheme.face)
    .popover(
      isPresented: Binding(
        get: { destinationKind != nil },
        set: { if !$0 { destinationKind = nil } })
    ) {
      destinationPopover
    }
  }

  private func fontButton(
    _ title: String, _ font: WritingFont, document: WritingDocument
  ) -> some View {
    Button {
      setFont(font)
    } label: {
      if document.font == font { Label(title, systemImage: "checkmark") } else { Text(title) }
    }
  }

  private func formatButton(_ title: String, _ kind: WritingFormatKind) -> some View {
    Button(title) { apply(kind) }
  }

  private func formatIcon(
    _ icon: String, _ help: String, _ kind: WritingFormatKind
  ) -> some View {
    Button {
      apply(kind)
    } label: {
      Image(systemName: icon).font(.system(size: 14, weight: .semibold)).frame(
        width: 26, height: 28)
    }.help(help)
  }

  private func destinationIcon(
    _ icon: String, _ help: String, _ kind: WritingFormatKind
  ) -> some View {
    Button {
      destination = ""
      destinationKind = kind
    } label: {
      Image(systemName: icon).font(.system(size: 14, weight: .semibold)).frame(
        width: 26, height: 28)
    }.help(help)
  }

  private var destinationPopover: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      Text(destinationKind == .image ? "Image path" : "Link destination")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
      TextField(destinationKind == .image ? "images/cover.png" : "https://", text: $destination)
        .textFieldStyle(.plain).font(MacTheme.mono(MacTheme.TypeScale.control))
        .padding(MacTheme.Space.x8).neoWell(radius: MacTheme.Radius.compact)
      HStack {
        Button("Cancel") { destinationKind = nil }
        Spacer()
        Button("Apply") {
          if let kind = destinationKind { apply(kind, value: destination) }
          destinationKind = nil
        }
        .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(destination.isEmpty)
      }
    }
    .padding(MacTheme.Space.x16).frame(width: 320).background(MacTheme.canvas)
  }

  private var wordCountLabel: String {
    let text = file?.viewport?.lines.map(\.text).joined(separator: "\n") ?? ""
    let count = text.split(whereSeparator: { $0.isWhitespace }).count
    return "\(count) words"
  }

  private func fontLabel(_ font: WritingFont) -> String {
    switch font {
    case .reading_serif: "Newsreader"
    case .classic_serif: "Classic serif"
    case .sans: "Sans"
    }
  }

  private func selectedRange() -> SoyehtProtocol.TextRange {
    selection
      ?? SoyehtProtocol.TextRange(
        start: TextPosition(line: activeLine, column: 0),
        end: TextPosition(line: activeLine, column: 0))
  }

  private func apply(_ kind: WritingFormatKind, value: String? = nil) {
    guard let fileId = writing?.document.file.fileId else { return }
    let range = selectedRange()
    Task { @MainActor in
      do {
        _ = try await client.formatWritingDocument(
          fileId: fileId, range: range, kind: kind, value: value)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func setFont(_ font: WritingFont) {
    guard let fileId = writing?.document.file.fileId else { return }
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, font: font) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func setFocus(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, focusMode: enabled) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func setFullWidth(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, fullWidth: enabled) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func loadIfNeeded() async {
    guard let writing, file?.viewport == nil else { return }
    do { try await client.loadWritingPane(writing.document.file.paneId) } catch {
      client.problem = error.localizedDescription
    }
  }
}

private struct WritingTextReviewStrip: View {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let selectedPosition: TextPosition?
  @State private var showingReview = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "person.crop.circle.badge.checkmark")
        .foregroundStyle(MacTheme.accent)
      Text("Agent changes ready to review")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
      Button("Review") { showingReview = true }
        .buttonStyle(NeoPillButtonStyle())
        .accessibilityIdentifier("writing-review-\(view.file.fileId)")
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: PaneLayoutContentMetrics.textReviewHeight)
    .background(MacTheme.canvas)
    .popover(isPresented: $showingReview, arrowEdge: .bottom) {
      EditorAgentReviewBar(
        client: client, view: view, selectedPosition: selectedPosition)
        .padding(MacTheme.Space.x10)
        .frame(width: 680)
        .background(MacTheme.canvas)
    }
  }
}

extension View {
  /// Places a writing surface in the plan's coordinate space without post-layout clipping.
  func writingFrame(_ rect: PaneLayoutRect) -> some View {
    frame(width: rect.size.width, height: rect.size.height, alignment: .topLeading)
      .position(
        x: rect.origin.x + rect.size.width / 2,
        y: rect.origin.y + rect.size.height / 2)
  }
}

private struct WritingTextViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  @Binding var selection: SoyehtProtocol.TextRange?
  @Binding var activeLine: UInt32

  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeNSView(context: Context) -> NSScrollView { context.coordinator.makeScrollView() }
  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(scrollView)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: WritingTextViewport
    private let textView = WritingImagePasteTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedRevision: String?
    private var renderedFont: WritingFont?
    private var renderedTheme: TerminalColorTheme?
    private var rendering = false

    init(_ parent: WritingTextViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.autohidesScrollers = true
      scroll.borderType = .noBorder
      scroll.documentView = textView
      configureTextView()
      return scroll
    }

    func update(_ scrollView: NSScrollView) {
      guard let viewport = parent.view.viewport else { return }
      applyTheme(scrollView)
      firstLine = viewport.fromLine
      let next = viewport.lines.map(\.text).joined(separator: "\n")
      let revision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if revision != renderedRevision || renderedFont != parent.document.font
        || renderedTheme != SoyehtThemeStore.shared.activeTheme
      {
        source = next
        renderedRevision = revision
        renderedFont = parent.document.font
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: validSelection(textView.selectedRange()))
      } else if parent.activeLine != currentLine() || parent.document.focusMode != focusApplied {
        render(selection: validSelection(textView.selectedRange()))
      }
    }

    private func applyTheme(_ scrollView: NSScrollView) {
      scrollView.backgroundColor = NSColor(MacTheme.face)
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !rendering else { return }
      let range = textView.selectedRange()
      guard let start = position(range.location), let end = position(NSMaxRange(range)) else {
        return
      }
      parent.selection = SoyehtProtocol.TextRange(start: start, end: end)
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
      let raw = source as NSString
      guard NSMaxRange(affectedCharRange) <= raw.length,
        let start = position(affectedCharRange.location),
        let end = position(NSMaxRange(affectedCharRange))
      else { return false }
      source = raw.replacingCharacters(in: affectedCharRange, with: replacement)
      let cursor = affectedCharRange.location + replacement.utf16.count
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId,
        edits: [
          TextEdit(range: SoyehtProtocol.TextRange(start: start, end: end), text: replacement)
        ])
      parent.activeLine = start.line &+ UInt32(clamping: replacement.filter { $0 == "\n" }.count)
      render(selection: NSRange(location: cursor, length: 0))
      return false
    }

    private var focusApplied: Bool {
      guard let storage = textView.textStorage, storage.length > 0 else { return false }
      return storage.attribute(.writingFocus, at: 0, effectiveRange: nil) as? Bool ?? false
    }

    private func currentLine() -> UInt32 {
      position(textView.selectedRange().location)?.line ?? firstLine
    }

    private func render(selection: NSRange) {
      rendering = true
      let projection = WritingMarkdownProjection.render(
        source: source, firstLine: firstLine, activeLine: parent.activeLine)
      let attributed = WritingTextStyle.attributed(
        projection, font: parent.document.font, focus: parent.document.focusMode)
      textView.textStorage?.setAttributedString(attributed)
      textView.setSelectedRange(validSelection(selection))
      textView.needsDisplay = true
      rendering = false
    }

    private func validSelection(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else {
        return NSRange(location: 0, length: 0)
      }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }

    private func position(_ offset: Int) -> TextPosition? {
      EditorEditing.textPosition(text: source, viewportStartLine: firstLine, utf16Offset: offset)
    }

    private func configureTextView() {
      textView.delegate = self
      textView.isRichText = false
      textView.importsGraphics = false
      textView.allowsUndo = true
      textView.usesFindPanel = true
      textView.isContinuousSpellCheckingEnabled = true
      textView.drawsBackground = true
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
      // The pure writing plan owns both the 66-character cap and the page inset.
      textView.textContainerInset = .zero
      textView.minSize = NSSize(width: 0, height: 0)
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: .greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = false
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = true
      textView.textContainer?.lineFragmentPadding = 0
      textView.registerForDraggedTypes([.png, .tiff, .fileURL])
      textView.imageHandler = { [weak self] pasteboard in
        self?.insertImage(from: pasteboard) ?? false
      }
    }

    private func insertImage(from pasteboard: NSPasteboard) -> Bool {
      guard let start = position(textView.selectedRange().location),
        let end = position(NSMaxRange(textView.selectedRange())),
        let image = NSImage(pasteboard: pasteboard)
      else { return false }
      let attachment: WebHostAttachment
      do { attachment = try WritingImageAttachment.make(from: image) } catch {
        parent.client.problem = error.localizedDescription
        return true
      }
      let range = SoyehtProtocol.TextRange(start: start, end: end)
      Task { @MainActor [parent] in
        do {
          _ = try await parent.client.formatWritingDocument(
            fileId: parent.view.file.fileId, range: range, kind: .image, image: attachment)
        } catch { parent.client.problem = error.localizedDescription }
      }
      return true
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
