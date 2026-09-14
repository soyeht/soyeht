import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private let screenplayElementKinds: [ScreenplayElementKind] = [
  .scene_heading, .action, .character, .parenthetical, .dialogue, .transition, .shot,
]

struct WritingScreenplayPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var railVisible = true
  @State private var editRequest: ScreenplayEditRequest?
  @State private var hoveredLine: UInt32?
  @State private var hoverY: CGFloat = 0
  @State private var showingBeatBoard = false

  private var writing: WritingDocumentView? {
    client.writingDocuments.values.first { $0.document.file.paneId == pane.paneId }
  }
  private var file: EditorFileView? {
    guard let fileId = writing?.document.file.fileId else { return nil }
    return client.editorFiles[fileId]
  }
  private var structure: WritingStructureResult? { writing?.structure }

  var body: some View {
    VStack(spacing: 0) {
      if showingBeatBoard, let writing, let structure {
        WritingBeatBoardPane(
          client: client, pane: pane, document: writing.document, structure: structure,
          showScreenplay: { showingBeatBoard = false })
      } else {
        header
        screenplayContent
      }
    }
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .task(id: writing?.document.file.fileId) { await loadIfNeeded() }
  }

  @ViewBuilder private var screenplayContent: some View {
    if let writing, let file, file.viewport != nil, let structure {
      HStack(spacing: 0) {
        if railVisible {
          ScreenplaySceneRail(
            scenes: structure.scenes, elements: structure.elements,
            activeLine: activeLine, select: selectScene)
          Divider().overlay(MacTheme.well)
        }
        VStack(spacing: 0) {
          toolbar(document: writing.document, structure: structure)
          screenplaySheet(file: file, document: writing.document, structure: structure)
        }
      }
    } else {
      ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
    }
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "doc.text")
        .foregroundStyle(MacTheme.secondary)
      Text(pane.name)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold)).lineLimit(1)
      Spacer()
      Button {
        showingBeatBoard = true
      } label: {
        Label("Idea board", systemImage: "lightbulb")
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      }
      .buttonStyle(NeoPillButtonStyle())
      .accessibilityIdentifier("writing-show-beat-board")
    }
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 42)
    .background(MacTheme.well, in: Capsule())
    .padding(.horizontal, MacTheme.Space.x10).padding(.top, MacTheme.Space.x10)
    .padding(.bottom, MacTheme.Space.x6)
  }

  private func toolbar(
    document: WritingDocument, structure: WritingStructureResult
  ) -> some View {
    VStack(spacing: 0) {
      HStack(spacing: MacTheme.Space.x10) {
        Button {
          railVisible.toggle()
        } label: {
          Image(systemName: railVisible ? "sidebar.left" : "sidebar.right")
            .frame(width: 28, height: 28)
        }
        .buttonStyle(.plain).help(railVisible ? "Collapse scenes" : "Show scenes")

        Divider().frame(height: 22)
        Menu {
          ForEach(screenplayElementKinds, id: \.self) { kind in
            Button(kind.label) { requestEdit(kind: kind) }
          }
        } label: {
          HStack(spacing: MacTheme.Space.x8) {
            Text(currentKind(in: structure).label)
            Image(systemName: "chevron.down")
          }
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
          .foregroundStyle(MacTheme.ink)
          .padding(.horizontal, MacTheme.Space.x12).frame(height: 30)
          .background(MacTheme.well, in: Capsule())
        }
        .menuStyle(.borderlessButton).frame(width: 176)

        Divider().frame(height: 22)
        Text("Tab and Enter advance the element")
          .font(MacTheme.chrome(MacTheme.TypeScale.control))
          .foregroundStyle(MacTheme.muted)
        Spacer(minLength: 0)
        Button {
          setFocus(!document.focusMode)
        } label: {
          Label("Focus", systemImage: "scope")
            .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
            .foregroundStyle(document.focusMode ? MacTheme.accent : MacTheme.secondary)
            .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
            .background(document.focusMode ? MacTheme.selection : Color.clear, in: Capsule())
        }
        .buttonStyle(.plain)
      }
      .padding(.horizontal, MacTheme.Space.x12).frame(height: 60).background(MacTheme.face)

      let suggestions = smartTypeSuggestions(in: structure)
      if !suggestions.isEmpty {
        HStack(spacing: MacTheme.Space.x6) {
          Text("SMARTTYPE")
            .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
            .foregroundStyle(MacTheme.muted)
          ForEach(suggestions, id: \.self) { suggestion in
            Button(suggestion) {
              requestEdit(kind: currentKind(in: structure), text: suggestion)
            }
            .buttonStyle(NeoPillButtonStyle())
            .font(MacTheme.screenplay(MacTheme.TypeScale.small, weight: .bold))
          }
          Spacer()
        }
        .padding(.horizontal, MacTheme.Space.x14).frame(height: 34)
        .background(MacTheme.canvas)
      }
    }
  }

  private func screenplaySheet(
    file: EditorFileView, document: WritingDocument, structure: WritingStructureResult
  ) -> some View {
    HStack(spacing: 0) {
      Spacer(minLength: 22)
      ZStack(alignment: .topLeading) {
        ScreenplayViewport(
          client: client, view: file, document: document, structure: structure,
          activeLine: $activeLine, targetLine: $targetLine, editRequest: $editRequest,
          hoveredLine: $hoveredLine, hoverY: $hoverY)
        if let variants = hoveredVariants(in: structure) {
          GeometryReader { geometry in
            ScreenplayVariantControl(
              client: client, pane: pane, fileId: file.file.fileId, variants: variants
            )
            .fixedSize()
            .position(
              x: max(150, geometry.size.width - 154),
              y: min(max(58, hoverY - 4), max(58, geometry.size.height - 58)))
          }
          .allowsHitTesting(true)
        }
      }
      .frame(maxWidth: 860, maxHeight: .infinity)
      .neoRaised(radius: MacTheme.Radius.control, elevation: .panel)
      .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
      .onHover { inside in
        if !inside { hoveredLine = nil }
      }
      Spacer(minLength: 22)
    }
    .padding(.vertical, 32).background(MacTheme.canvas)
  }

  private func currentKind(in structure: WritingStructureResult) -> ScreenplayElementKind {
    ScreenplayInteraction.element(at: activeLine, in: structure)?.kind ?? .action
  }

  private func smartTypeSuggestions(in structure: WritingStructureResult) -> [String] {
    let element = ScreenplayInteraction.element(at: activeLine, in: structure)
    return Array(
      ScreenplayInteraction.suggestions(
        for: element?.kind ?? .action, prefix: element?.text ?? "", in: structure
      ).prefix(5))
  }

  private func hoveredVariants(
    in structure: WritingStructureResult
  ) -> WritingLineVariants? {
    guard let hoveredLine else { return nil }
    return ScreenplayInteraction.variants(at: hoveredLine, in: structure)
  }

  private func requestEdit(kind: ScreenplayElementKind, text: String? = nil) {
    editRequest = ScreenplayEditRequest(kind: kind, text: text)
  }

  private func setFocus(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, focusMode: enabled) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func selectScene(_ scene: ScreenplayScene) {
    targetLine = scene.line
    guard let fileId = writing?.document.file.fileId, let viewport = file?.viewport,
      scene.line < viewport.fromLine || scene.line >= viewport.toLine
    else { return }
    Task { @MainActor in
      do { try await client.loadEditorViewport(fileId: fileId, fromLine: scene.line) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func loadIfNeeded() async {
    guard let writing else { return }
    do { try await client.loadWritingPane(writing.document.file.paneId) } catch {
      client.problem = error.localizedDescription
    }
  }
}

private struct ScreenplaySceneRail: View {
  let scenes: [ScreenplayScene]
  let elements: [ScreenplayElement]
  let activeLine: UInt32
  let select: (ScreenplayScene) -> Void

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack {
        Text("SCENES")
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
          .tracking(1).foregroundStyle(MacTheme.secondary)
        Spacer()
        Text("\(scenes.count)")
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
      }
      ScrollView {
        LazyVStack(spacing: MacTheme.Space.x7) {
          ForEach(scenes, id: \.sceneId) { scene in
            sceneButton(scene)
          }
        }
      }
    }
    .padding(MacTheme.Space.x12).frame(width: 330).background(MacTheme.canvas)
  }

  private func sceneButton(_ scene: ScreenplayScene) -> some View {
    let selected = selectedScene?.sceneId == scene.sceneId
    return Button {
      select(scene)
    } label: {
      VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
        HStack(alignment: .firstTextBaseline, spacing: MacTheme.Space.x7) {
          Text("\(scene.ordinal + 1)")
            .font(MacTheme.mono(MacTheme.TypeScale.control, weight: .bold))
            .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
          Text(scene.heading)
            .font(MacTheme.screenplay(MacTheme.TypeScale.caption, weight: .bold))
            .foregroundStyle(MacTheme.ink).lineLimit(2)
          Spacer(minLength: 2)
          Text("p. \(page(for: scene))")
            .font(MacTheme.chrome(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
        }
        if let summary = summary(for: scene) {
          Text(summary)
            .font(MacTheme.chrome(MacTheme.TypeScale.small))
            .foregroundStyle(MacTheme.secondary).lineLimit(2)
        }
        if !scene.linkedBeatIds.isEmpty {
          Label(
            "\(scene.linkedBeatIds.count) \(scene.linkedBeatIds.count == 1 ? "idea" : "ideas")",
            systemImage: "link"
          )
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.accent)
        }
      }
      .padding(MacTheme.Space.x10).frame(maxWidth: .infinity, alignment: .leading)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
      .overlay {
        RoundedRectangle(cornerRadius: MacTheme.Radius.control)
          .stroke(selected ? MacTheme.accent : Color.clear, lineWidth: 2)
      }
    }
    .buttonStyle(.plain)
  }

  private var selectedScene: ScreenplayScene? {
    scenes.last { $0.line <= activeLine }
  }

  private func summary(for scene: ScreenplayScene) -> String? {
    elements.first { $0.sceneId == scene.sceneId && $0.kind == .action }?.text
  }

  private func page(for scene: ScreenplayScene) -> UInt32 { scene.line / 55 + 1 }
}

private struct ScreenplayVariantControl: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let fileId: String
  let variants: WritingLineVariants

  private var selected: WritingVariant? {
    variants.alternates[safe: Int(variants.selectedIndex)]
  }

  var body: some View {
    VStack(alignment: .trailing, spacing: MacTheme.Space.x6) {
      if let selected {
        Text("✨ \(selected.author.name) wrote \(variants.alternates.count) versions")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.face)
          .padding(.horizontal, MacTheme.Space.x8).frame(height: 24)
          .background(MacTheme.ink, in: Capsule())
      }
      HStack(spacing: MacTheme.Space.x6) {
        variantButton("chevron.left") { cycle(-1) }
        Text("\(Int(variants.selectedIndex) + 1) / \(variants.alternates.count)")
          .font(MacTheme.screenplay(MacTheme.TypeScale.control, weight: .bold))
          .foregroundStyle(MacTheme.ink)
        variantButton("chevron.right") { cycle(1) }
        Divider().frame(height: 20)
        variantButton("plus") { requestAnother() }
        variantButton("minus") { removeSelected() }
          .disabled(variants.alternates.count <= 1)
      }
      .padding(.horizontal, MacTheme.Space.x8).frame(height: 34)
      .background(MacTheme.face, in: Capsule()).neoRaised(radius: MacTheme.Radius.pill)
    }
  }

  private func variantButton(_ icon: String, action: @escaping () -> Void) -> some View {
    Button(action: action) {
      Image(systemName: icon).frame(width: 20, height: 24)
    }
    .buttonStyle(.plain).foregroundStyle(MacTheme.accent)
  }

  private func cycle(_ delta: Int) {
    guard let variant = ScreenplayInteraction.adjacentVariant(in: variants, delta: delta) else {
      return
    }
    Task { @MainActor in
      do {
        _ = try await client.selectWritingVariant(
          fileId: fileId, lineId: variants.lineId, variantId: variant.variantId)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func requestAnother() {
    guard
      let agent = client.panes.first(where: {
        $0.workspaceId == pane.workspaceId && $0.mode == .agent && $0.state != .dead
      })
    else {
      client.problem = "Open an agent pane in this workspace to request another take."
      return
    }
    Task { @MainActor in
      do {
        _ = try await client.requestWritingVariant(
          fileId: fileId, lineId: variants.lineId, agentPaneId: agent.paneId,
          instruction: "Write one alternate take for this screenplay line.")
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func removeSelected() {
    guard variants.alternates.count > 1, let selected else { return }
    Task { @MainActor in
      do {
        _ = try await client.removeWritingVariant(
          fileId: fileId, lineId: variants.lineId, variantId: selected.variantId)
      } catch { client.problem = error.localizedDescription }
    }
  }
}

private struct ScreenplayEditRequest: Equatable {
  let id = UUID()
  let kind: ScreenplayElementKind
  let text: String?

  init(kind: ScreenplayElementKind, text: String? = nil) {
    self.kind = kind
    self.text = text
  }
}

private struct ScreenplayViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  let structure: WritingStructureResult
  @Binding var activeLine: UInt32
  @Binding var targetLine: UInt32?
  @Binding var editRequest: ScreenplayEditRequest?
  @Binding var hoveredLine: UInt32?
  @Binding var hoverY: CGFloat

  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeNSView(context: Context) -> NSScrollView { context.coordinator.makeScrollView() }
  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(scrollView)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: ScreenplayViewport
    private let textView = ScreenplayTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedRevision: String?
    private var renderedTheme: TerminalColorTheme?
    private var applying = false
    private var appliedRequest: UUID?

    init(_ parent: ScreenplayViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.autohidesScrollers = true
      scroll.borderType = .noBorder
      scroll.documentView = textView
      configure(scroll)
      return scroll
    }

    func update(_ scrollView: NSScrollView) {
      guard let viewport = parent.view.viewport else { return }
      applyTheme(scrollView)
      firstLine = viewport.fromLine
      let next = viewport.lines.map(\.text).joined(separator: "\n")
      let revision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if revision != renderedRevision || renderedTheme != SoyehtThemeStore.shared.activeTheme {
        source = next
        renderedRevision = revision
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: valid(textView.selectedRange()))
      } else if parent.activeLine != currentLine() || focusApplied != parent.document.focusMode {
        render(selection: valid(textView.selectedRange()))
      }
      if let request = parent.editRequest, request.id != appliedRequest {
        appliedRequest = request.id
        apply(request)
        parent.editRequest = nil
      }
      if let line = parent.targetLine, line >= viewport.fromLine, line < viewport.toLine {
        scroll(to: line)
        parent.targetLine = nil
      }
      textView.hoverHandler = { [weak self, weak scrollView] point in
        guard let self, let scrollView else { return }
        hover(at: point, scroll: scrollView)
      }
    }

    private func applyTheme(_ scrollView: NSScrollView) {
      scrollView.backgroundColor = NSColor(MacTheme.face)
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !applying, let position = position(textView.selectedRange().location) else { return }
      if parent.activeLine != position.line {
        parent.activeLine = position.line
        render(selection: textView.selectedRange())
      }
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
      position(textView.selectedRange().location)?.line ?? firstLine
    }

    private func advance(tab: Bool) {
      let line = currentLine()
      let kind = ScreenplayInteraction.element(at: line, in: parent.structure)?.kind ?? .action
      let next = ScreenplayInteraction.nextKind(after: kind, tab: tab)
      if tab {
        replaceCurrentLine(with: nil, as: next)
      } else {
        let seed = ScreenplayInteraction.formattedLine("", as: next)
        replace(textView.selectedRange(), with: "\n\n" + seed)
      }
    }

    private func apply(_ request: ScreenplayEditRequest) {
      replaceCurrentLine(with: request.text, as: request.kind)
    }

    private func replaceCurrentLine(with text: String?, as kind: ScreenplayElementKind) {
      let range = (source as NSString).lineRange(
        for: NSRange(location: textView.selectedRange().location, length: 0))
      let contentRange = lineContentRange(range)
      let current = (source as NSString).substring(with: contentRange)
      let formatted = ScreenplayInteraction.formattedLine(text ?? current, as: kind)
      replace(contentRange, with: formatted)
    }

    private func replace(_ range: NSRange, with replacement: String) {
      let raw = source as NSString
      guard NSMaxRange(range) <= raw.length, let start = position(range.location),
        let end = position(NSMaxRange(range))
      else { return }
      source = raw.replacingCharacters(in: range, with: replacement)
      let cursor = range.location + replacement.utf16.count
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId,
        edits: [TextEdit(range: .init(start: start, end: end), text: replacement)])
      parent.activeLine = start.line &+ UInt32(clamping: replacement.filter { $0 == "\n" }.count)
      render(selection: NSRange(location: cursor, length: 0))
    }

    private func render(selection: NSRange) {
      applying = true
      textView.textStorage?.setAttributedString(
        ScreenplayStyle.attributed(
          source, firstLine: firstLine, activeLine: parent.activeLine,
          focus: parent.document.focusMode, structure: parent.structure))
      textView.setSelectedRange(valid(selection))
      textView.needsDisplay = true
      applying = false
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
      guard let position = position(character),
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
      let relative = Int(line - firstLine)
      let lines = source.components(separatedBy: "\n")
      guard relative >= 0, relative < lines.count else { return }
      let location = lines.prefix(relative).reduce(0) { $0 + $1.utf16.count + 1 }
      textView.setSelectedRange(NSRange(location: location, length: 0))
      textView.scrollRangeToVisible(NSRange(location: location, length: 0))
      parent.activeLine = line
    }

    private func position(_ offset: Int) -> TextPosition? {
      EditorEditing.textPosition(text: source, viewportStartLine: firstLine, utf16Offset: offset)
    }

    private func lineContentRange(_ lineRange: NSRange) -> NSRange {
      let raw = source as NSString
      var length = lineRange.length
      while length > 0 {
        let value = raw.character(at: lineRange.location + length - 1)
        if value != 10 && value != 13 { break }
        length -= 1
      }
      return NSRange(location: lineRange.location, length: length)
    }

    private func valid(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else { return .init() }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }

    private func configure(_ scroll: NSScrollView) {
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
      textView.textContainerInset = NSSize(width: 74, height: 58)
      textView.minSize = .zero
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
      textView.isVerticallyResizable = true
      textView.isHorizontallyResizable = false
      textView.autoresizingMask = [.width]
      textView.textContainer?.widthTracksTextView = true
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
    _ source: String, firstLine: UInt32, activeLine: UInt32, focus: Bool,
    structure: WritingStructureResult
  ) -> NSAttributedString {
    let paragraph = baseParagraph()
    let result = NSMutableAttributedString(
      string: source,
      attributes: [
        .font: MacTheme.screenplayNSFont(16), .foregroundColor: NSColor(MacTheme.ink),
        .paragraphStyle: paragraph, .screenplayFocus: focus,
      ])
    for element in structure.elements {
      guard
        let range = range(
          from: element.startLine, through: element.endLine, source: source, firstLine: firstLine)
      else { continue }
      style(element.kind, range: range, in: result)
    }
    for variants in structure.variants {
      guard
        let range = range(
          from: variants.line, through: variants.line, source: source, firstLine: firstLine)
      else { continue }
      result.addAttributes(
        [
          .underlineStyle: NSUnderlineStyle.single.union(.patternDot).rawValue,
          .underlineColor: NSColor(MacTheme.accent),
        ], range: range)
    }
    if focus {
      dimInactiveLines(result, source: source, firstLine: firstLine, activeLine: activeLine)
    }
    return result
  }

  private static func style(
    _ kind: ScreenplayElementKind, range: NSRange, in text: NSMutableAttributedString
  ) {
    let paragraph = baseParagraph()
    switch kind {
    case .scene_heading:
      paragraph.firstLineHeadIndent = 0
      paragraph.headIndent = 0
      text.addAttribute(.font, value: MacTheme.screenplayNSFont(16, bold: true), range: range)
    case .action, .shot:
      paragraph.firstLineHeadIndent = 0
      paragraph.headIndent = 0
      if kind == .shot {
        text.addAttribute(.font, value: MacTheme.screenplayNSFont(16, bold: true), range: range)
      }
    case .character:
      paragraph.firstLineHeadIndent = 250
      paragraph.headIndent = 250
      text.addAttribute(.font, value: MacTheme.screenplayNSFont(16, bold: true), range: range)
    case .parenthetical:
      paragraph.firstLineHeadIndent = 205
      paragraph.headIndent = 205
      paragraph.tailIndent = -150
      text.addAttribute(.foregroundColor, value: NSColor(MacTheme.secondary), range: range)
    case .dialogue:
      paragraph.firstLineHeadIndent = 145
      paragraph.headIndent = 145
      paragraph.tailIndent = -110
    case .transition:
      paragraph.alignment = .right
      text.addAttribute(.foregroundColor, value: NSColor(MacTheme.secondary), range: range)
    }
    text.addAttribute(.paragraphStyle, value: paragraph, range: range)
  }

  private static func baseParagraph() -> NSMutableParagraphStyle {
    let style = NSMutableParagraphStyle()
    style.minimumLineHeight = 27
    style.maximumLineHeight = 27
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

  private static func dimInactiveLines(
    _ text: NSMutableAttributedString, source: String, firstLine: UInt32, activeLine: UInt32
  ) {
    guard activeLine >= firstLine else { return }
    let lines = source.components(separatedBy: "\n")
    let active = Int(activeLine - firstLine)
    var location = 0
    for (index, line) in lines.enumerated() {
      if index != active, !line.isEmpty {
        text.addAttribute(
          .foregroundColor, value: NSColor(MacTheme.muted),
          range: NSRange(location: location, length: line.utf16.count))
      }
      location += line.utf16.count + 1
    }
  }
}

extension ScreenplayElementKind {
  fileprivate var label: String {
    switch self {
    case .scene_heading: "Scene heading"
    case .action: "Action"
    case .character: "Character"
    case .parenthetical: "Parenthetical"
    case .dialogue: "Dialogue"
    case .transition: "Transition"
    case .shot: "Shot"
    }
  }
}

extension NSAttributedString.Key {
  fileprivate static let screenplayFocus = NSAttributedString.Key("soyeht.screenplay.focus")
}

extension Collection {
  fileprivate subscript(safe index: Index) -> Element? {
    indices.contains(index) ? self[index] : nil
  }
}
