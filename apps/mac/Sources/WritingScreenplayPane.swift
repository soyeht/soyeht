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
  let headerDrag: PaneHeaderDragConfiguration?
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var showingSceneDrawer = false
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
  private var writingLoadKey: String {
    writing?.loadIdentity ?? "missing"
  }

  var body: some View {
    Group {
      if showingBeatBoard, let writing, let structure {
        WritingBeatBoardPane(
          client: client, pane: pane, document: writing.document, structure: structure,
          showScreenplay: { showingBeatBoard = false }
        )
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
      } else {
        GeometryReader { proxy in
          if let writing, let file, file.viewport != nil, let structure,
            let plan = layoutPlan(
              size: proxy.size, document: writing.document, structure: structure),
            case .screenplay(let screenplayPlan) = plan.surface
          {
            ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
              plannedSurface(
                plan: plan, screenplayPlan: screenplayPlan, file: file,
                document: writing.document, structure: structure)
            }
            .scrollBounceBehavior(.basedOnSize)
            .scrollDisabled(plan.overflowAxes.isEmpty)
            .frame(width: proxy.size.width, height: proxy.size.height)
          } else {
            VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
              screenplayHeader
              ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .padding(PaneLayoutContentMetrics.paneCardInset)
            .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
          }
        }
      }
    }
    .task(id: writingLoadKey) { await loadIfNeeded() }
  }

  private func layoutPlan(
    size: CGSize, document: WritingDocument, structure: WritingStructureResult
  ) -> WritingLayoutPlan? {
    let policy = WritingLayoutPolicy(
      floors: MacPaneLayoutMetrics.surfaceFloors,
      gaps: MacPaneLayoutMetrics.sizing.gaps)
    return try? policy.layout(
      WritingLayoutRequest(
        kind: .screenplay,
        viewport: PaneLayoutSize(width: size.width, height: size.height),
        font: MacPaneLayoutMetrics.screenplayMetric,
        smartTypeVisible: !smartTypeSuggestions(in: structure).isEmpty))
  }

  private func plannedSurface(
    plan: WritingLayoutPlan, screenplayPlan: WritingScreenplayLayoutPlan,
    file: EditorFileView, document: WritingDocument, structure: WritingStructureResult
  ) -> some View {
    ZStack(alignment: .topLeading) {
      Color.clear
        .frame(width: plan.contentSize.width, height: plan.contentSize.height)
        .allowsHitTesting(false)
      MacTheme.canvas
        .writingFrame(plan.contentRect)
      screenplayHeader
        .writingFrame(plan.headerRect)
      if let toolbarRect = plan.toolbarRect {
        toolbar(
          document: document, structure: structure,
          navigation: screenplayPlan.navigation
        )
        .writingFrame(toolbarRect)
      }
      if let navigationRect = screenplayPlan.navigationRect {
        ScreenplaySceneRail(
          scenes: structure.scenes, elements: structure.elements,
          activeLine: activeLine, select: selectScene
        )
        .writingFrame(navigationRect)
        .accessibilityIdentifier("writing-screenplay-scene-rail-\(pane.paneId)")
        MacTheme.well
          .writingFrame(
            PaneLayoutRect(
              origin: PaneLayoutPoint(
                x: navigationRect.origin.x + navigationRect.size.width,
                y: navigationRect.origin.y),
              size: PaneLayoutSize(
                width: WritingLayoutMetrics.standard.railDividerWidth,
                height: navigationRect.size.height)))
      }
      screenplayPage(
        file: file, document: document, structure: structure, plan: screenplayPlan
      )
      .writingFrame(screenplayPlan.pageRect)
    }
    .frame(
      width: plan.contentSize.width, height: plan.contentSize.height,
      alignment: .topLeading
    )
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
  }

  private var screenplayHeader: some View {
    WritingPaneHeader(
      paneId: pane.paneId, title: pane.name, icon: "doc.text", metadata: nil,
      fullWidth: nil, headerDrag: headerDrag,
      auxiliaryAction: WritingPaneHeaderAction(
        title: "Idea board", systemImage: "lightbulb", help: "Show the idea board",
        accessibilityIdentifier: "writing-show-beat-board-\(pane.paneId)",
        action: { showingBeatBoard = true }),
      mutationDisabled: !client.connected,
      layoutActionsDisabled: !client.connected || layoutActionsDisabled,
      onToggleFullWidth: nil, onSplit: onSplit, onClose: onClose)
  }

  private func scrollAxes(_ plan: WritingLayoutPlan) -> Axis.Set {
    var axes: Axis.Set = []
    if plan.overflowAxes.contains(.horizontal) { axes.insert(.horizontal) }
    if plan.overflowAxes.contains(.vertical) { axes.insert(.vertical) }
    return axes
  }

  private func toolbar(
    document: WritingDocument, structure: WritingStructureResult,
    navigation: WritingSceneNavigationMode
  ) -> some View {
    VStack(spacing: 0) {
      ScrollView(.horizontal) {
        HStack(spacing: MacTheme.Space.x10) {
          if navigation == .drawer {
            Button {
              showingSceneDrawer.toggle()
            } label: {
              Image(systemName: "sidebar.left")
                .frame(width: 28, height: 28)
            }
            .buttonStyle(.plain)
            .help("Show scenes")
            .accessibilityLabel("Show scenes")
            .accessibilityIdentifier("writing-show-scenes-\(pane.paneId)")
            .popover(isPresented: $showingSceneDrawer, arrowEdge: .bottom) {
              ScreenplaySceneRail(
                scenes: structure.scenes, elements: structure.elements,
                activeLine: activeLine, select: selectScene
              )
              .frame(height: PaneLayoutContentMetrics.screenplaySceneRailWidth)
            }
            Divider().frame(height: 22)
          }

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
        .fixedSize(horizontal: true, vertical: false)
        .padding(.horizontal, MacTheme.Space.x12)
      }
      .scrollIndicators(.hidden)
      .frame(height: PaneLayoutContentMetrics.screenplayToolbarHeight)
      .background(MacTheme.face)

      let suggestions = smartTypeSuggestions(in: structure)
      if !suggestions.isEmpty {
        ScrollView(.horizontal) {
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
          }
          .fixedSize(horizontal: true, vertical: false)
          .padding(.horizontal, MacTheme.Space.x14)
        }
        .scrollIndicators(.hidden)
        .frame(height: PaneLayoutContentMetrics.screenplaySmartTypeHeight)
        .background(MacTheme.canvas)
      }
    }
  }

  private func screenplayPage(
    file: EditorFileView, document: WritingDocument, structure: WritingStructureResult,
    plan: WritingScreenplayLayoutPlan
  ) -> some View {
    ZStack(alignment: .topLeading) {
      ScreenplayViewport(
        client: client, view: file, document: document, structure: structure,
        pageSize: plan.pageSize, scale: plan.scale,
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
            y: min(
              max(58, hoverY * plan.scale - 4),
              max(58, geometry.size.height - 58)))
        }
        .allowsHitTesting(true)
      }
    }
    .neoRaised(radius: MacTheme.Radius.control, elevation: .panel)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    .onHover { inside in
      if !inside { hoveredLine = nil }
    }
    .accessibilityIdentifier("writing-screenplay-page-\(pane.paneId)")
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
          Text("\(identifier: scene.ordinal + 1)")
            .font(MacTheme.mono(MacTheme.TypeScale.control, weight: .bold))
            .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
          Text(scene.heading)
            .font(MacTheme.screenplay(MacTheme.TypeScale.caption, weight: .bold))
            .foregroundStyle(MacTheme.ink).lineLimit(2)
          Spacer(minLength: 2)
          Text("p. \(identifier: page(for: scene))")
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
        Text(
          "\(identifier: Int(variants.selectedIndex) + 1) / \(variants.alternates.count)"
        )
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
  let pageSize: PaneLayoutSize
  let scale: Double
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
      scroll.scrollerStyle = .overlay
      scroll.borderType = .noBorder
      scroll.documentView = textView
      configure(scroll)
      applyPageGeometry(scroll)
      return scroll
    }

    func update(_ scrollView: NSScrollView) {
      guard let viewport = parent.view.viewport else { return }
      applyTheme(scrollView)
      applyPageGeometry(scrollView)
      firstLine = viewport.fromLine
      let next = viewport.lines.map(\.text).joined(separator: "\n")
      let revision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if revision != renderedRevision || renderedTheme != SoyehtThemeStore.shared.activeTheme {
        source = next
        renderedRevision = revision
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: valid(textView.selectedRange()))
      } else if parent.activeLine != currentLine() || focusApplied != parent.document.focusMode {
        restyle(selection: valid(textView.selectedRange()))
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
      guard !applying, let position = position(textView.selectedRange().location) else { return }
      if parent.activeLine != position.line {
        parent.activeLine = position.line
        restyle(selection: textView.selectedRange())
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
      guard
        let edit = ScreenplayInteraction.advanceEdit(
          source: source, cursor: textView.selectedRange().location, kind: kind, tab: tab)
      else { return }
      replace(edit.range, with: edit.replacement)
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
      guard
        let edit = ScreenplayInteraction.typingEdit(
          source: source, firstLine: firstLine, range: range, replacement: replacement)
      else { return }
      source = edit.source
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
          source, firstLine: firstLine, activeLine: parent.activeLine,
          focus: parent.document.focusMode, structure: parent.structure))
      textView.setSelectedRange(valid(selection))
      textView.needsDisplay = true
      applying = false
    }

    private func restyle(selection: NSRange) {
      guard let storage = textView.textStorage, storage.string == source else {
        render(selection: selection)
        return
      }
      let styled = ScreenplayStyle.attributed(
        source, firstLine: firstLine, activeLine: parent.activeLine,
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
      textView.textContainerInset = NSSize(
        width: PaneLayoutContentMetrics.screenplayPageHorizontalInset, height: 58)
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
        .font: MacTheme.screenplayNSFont(PaneLayoutContentMetrics.screenplayFontPointSize),
        .foregroundColor: NSColor(MacTheme.ink),
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
    let line = text.attributedSubstring(from: range).string
    let projection = ScreenplayInteraction.lineProjection(source: line, kind: kind)
    switch kind {
    case .scene_heading:
      paragraph.firstLineHeadIndent = 0
      paragraph.headIndent = 0
      text.addAttribute(
        .font,
        value: MacTheme.screenplayNSFont(
          PaneLayoutContentMetrics.screenplayFontPointSize, bold: true),
        range: range)
    case .action, .shot:
      paragraph.firstLineHeadIndent = 0
      paragraph.headIndent = 0
      if kind == .shot {
        text.addAttribute(
          .font,
          value: MacTheme.screenplayNSFont(
            PaneLayoutContentMetrics.screenplayFontPointSize, bold: true),
          range: range)
      }
    case .character:
      paragraph.alignment = .center
      text.addAttribute(
        .font,
        value: MacTheme.screenplayNSFont(
          PaneLayoutContentMetrics.screenplayFontPointSize, bold: true),
        range: range)
    case .parenthetical:
      paragraph.alignment = .center
      text.addAttribute(.foregroundColor, value: NSColor(MacTheme.secondary), range: range)
    case .dialogue:
      paragraph.firstLineHeadIndent = 120
      paragraph.headIndent = 120
      paragraph.tailIndent = -120
    case .transition:
      paragraph.alignment = projection.openingTransition ? .left : .right
      text.addAttribute(.foregroundColor, value: NSColor(MacTheme.secondary), range: range)
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
          .font: MacTheme.screenplayNSFont(0.1),
          .foregroundColor: NSColor(MacTheme.face),
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
