import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private let screenplayElementKinds: [ScreenplayElementKind] = [
  .scene_heading, .action, .character, .parenthetical, .dialogue, .transition, .shot,
]

private enum ScreenplayHeaderPopover: String, Identifiable {
  case options
  case scenes

  var id: String { rawValue }
}

struct WritingScreenplayPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var headerPopover: ScreenplayHeaderPopover?
  @State private var showingCloseConfirmation = false
  @State private var chromeActivity = WritingChromeActivityState()
  @State private var elementMenuAnchor: PaneLayoutPoint?
  @State private var showingElementMenu = false
  @State private var showingElementKindPicker = false
  @State private var smartTypeKind: ScreenplayElementKind = .action
  @State private var smartTypeSuggestions: [String] = []
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
            let plan = layoutPlan(size: proxy.size),
            case .screenplay(let screenplayPlan) = plan.surface
          {
            ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
              plannedSurface(
                plan: plan, screenplayPlan: screenplayPlan, file: file,
                document: writing.document, structure: structure
              )
              .forceEnclosingOverlayScrollers()
            }
            .scrollBounceBehavior(.basedOnSize)
            .scrollDisabled(plan.overflowAxes.isEmpty)
            .frame(width: proxy.size.width, height: proxy.size.height)
            .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
            .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
          } else {
            VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
              screenplayHeader(document: writing?.document, structure: structure)
              ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .padding(PaneLayoutContentMetrics.paneCardInset)
            .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
          }
        }
      }
    }
    .task(id: writingLoadKey) { await loadIfNeeded() }
    .onChange(of: headerPopover != nil) { _, value in
      updateChrome(.menuChanged(value))
    }
    .onChange(of: elementMenuAnchor == nil) { _, hidden in
      if hidden { showingElementKindPicker = false }
    }
    .alert("Close \(documentFileName)?", isPresented: $showingCloseConfirmation) {
      Button("Cancel", role: .cancel) {}
      Button("Close", role: .destructive) { client.closePane(pane.paneId) }
    } message: {
      Text("Unsaved changes will be lost.")
    }
  }

  private func layoutPlan(size: CGSize) -> WritingLayoutPlan? {
    let policy = WritingLayoutPolicy(
      floors: MacPaneLayoutMetrics.surfaceFloors,
      gaps: MacPaneLayoutMetrics.sizing.gaps)
    return try? policy.layout(
      WritingLayoutRequest(
        kind: .screenplay,
        viewport: PaneLayoutSize(width: size.width, height: size.height),
        font: MacPaneLayoutMetrics.screenplayMetric))
  }

  private func plannedSurface(
    plan: WritingLayoutPlan, screenplayPlan: WritingScreenplayLayoutPlan,
    file: EditorFileView, document: WritingDocument, structure: WritingStructureResult
  ) -> some View {
    ZStack(alignment: .topLeading) {
      Color.clear
        .frame(width: plan.contentSize.width, height: plan.contentSize.height)
        .allowsHitTesting(false)
      MacTheme.face
        .writingFrame(plan.contentRect)
      screenplayHeader(document: document, structure: structure)
        .writingFrame(plan.headerRect)
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
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .onContinuousHover { phase in
      if case .active = phase { updateChrome(.pointerMoved) }
    }
  }

  private func screenplayHeader(
    document: WritingDocument?, structure: WritingStructureResult?
  ) -> some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: documentFileName,
      metadata: structure.map { sceneCountLabel($0.scenes.count) },
      companionLinked: companionLinked, opacity: chromeOpacity,
      headerDrag: headerDrag,
      onHover: { updateChrome(.headerHoverChanged($0)) },
      trailing: {
        Button {
          headerPopover = headerPopover == nil ? .options : nil
        } label: {
          Image(systemName: "ellipsis")
            .font(.system(size: WritingChromeMetrics.standard.headerMenuIconSize))
            .foregroundStyle(MacTheme.secondary)
            .frame(
              width: WritingChromeMetrics.standard.headerMenuTargetSize,
              height: WritingChromeMetrics.standard.headerMenuTargetSize
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("Screenplay options")
        .accessibilityLabel("Screenplay options menu")
        .accessibilityIdentifier("writing-options-\(pane.paneId)")
        .popover(item: $headerPopover, arrowEdge: .bottom) { popover in
          switch popover {
          case .options:
            if let document {
              screenplayOptions(document: document)
            }
          case .scenes:
            if let structure {
              ScreenplaySceneRail(
                scenes: structure.scenes, elements: structure.elements,
                activeLine: activeLine, select: selectScene
              )
              .frame(height: PaneLayoutContentMetrics.screenplaySceneDrawerHeight)
            }
          }
        }
      }
    )
  }

  private func scrollAxes(_ plan: WritingLayoutPlan) -> Axis.Set {
    var axes: Axis.Set = []
    if plan.overflowAxes.contains(.horizontal) { axes.insert(.horizontal) }
    if plan.overflowAxes.contains(.vertical) { axes.insert(.vertical) }
    return axes
  }

  private func screenplayOptions(document: WritingDocument) -> some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      optionButton(
        "Idea board", icon: "lightbulb",
        identifier: "writing-show-beat-board-\(pane.paneId)"
      ) {
        headerPopover = nil
        showingBeatBoard = true
      }
      optionButton(
        "Scenes", icon: "sidebar.left",
        identifier: "writing-show-scenes-\(pane.paneId)"
      ) {
        headerPopover = .scenes
      }
      optionButton(
        document.focusMode ? "Turn off Focus mode" : "Focus mode", icon: "scope",
        active: document.focusMode, disabled: !client.connected,
        identifier: "writing-options-focus-\(pane.paneId)"
      ) {
        headerPopover = nil
        setFocus(!document.focusMode)
      }
      Divider().padding(.vertical, MacTheme.Space.x3)
      optionButton(
        "Split into columns", icon: "rectangle.split.2x1",
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "split-pane-columns-\(pane.paneId)"
      ) {
        headerPopover = nil
        onSplit(pane.paneId, .columns)
      }
      optionButton(
        "Split into rows", icon: "rectangle.split.1x2",
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "split-pane-rows-\(pane.paneId)"
      ) {
        headerPopover = nil
        onSplit(pane.paneId, .rows)
      }
      Divider().padding(.vertical, MacTheme.Space.x3)
      optionButton(
        "Close pane", icon: "xmark", role: .destructive,
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "close-pane-\(pane.paneId)"
      ) {
        headerPopover = nil
        requestClose()
      }
    }
    .padding(MacTheme.Space.x8)
    .frame(width: PaneLayoutContentMetrics.writingOptionsMenuWidth)
    .background(MacTheme.face)
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Screenplay options menu")
  }

  private func optionLabel(_ title: String, icon: String, active: Bool = false) -> some View {
    Label(title, systemImage: icon)
      .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      .foregroundStyle(active ? MacTheme.accent : MacTheme.ink)
      .frame(
        maxWidth: .infinity,
        minHeight: PaneLayoutContentMetrics.writingOptionsRowHeight,
        alignment: .leading
      )
      .contentShape(Rectangle())
  }

  private func optionButton(
    _ title: String, icon: String, role: ButtonRole? = nil, active: Bool = false,
    disabled: Bool = false, identifier: String, action: @escaping () -> Void
  ) -> some View {
    Button(role: role, action: action) { optionLabel(title, icon: icon, active: active) }
      .buttonStyle(.plain)
      .focusable(false)
      .disabled(disabled)
      .accessibilityIdentifier(identifier)
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
        hoveredLine: $hoveredLine, hoverY: $hoverY,
        elementMenuAnchor: $elementMenuAnchor, showingElementMenu: $showingElementMenu,
        smartTypeKind: $smartTypeKind, smartTypeSuggestions: $smartTypeSuggestions,
        onEditingActivity: { updateChrome(.edited) })
      if let elementMenuAnchor {
        let controlsWidth = max(0, plan.pageRect.size.width - elementMenuAnchor.x)
        screenplayInlineControls(availableWidth: controlsWidth)
          .frame(
            width: controlsWidth,
            height: PaneLayoutContentMetrics.screenplayInlineElementMenuHeight
          )
          .position(x: elementMenuAnchor.x + controlsWidth / 2, y: elementMenuAnchor.y)
          .zIndex(2)
      }
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

  private func screenplayInlineControls(availableWidth: CGFloat) -> some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: PaneLayoutContentMetrics.screenplayInlineControlGap) {
        if showingElementMenu { screenplayElementMenu() }
        if !smartTypeSuggestions.isEmpty {
          Text("SMARTTYPE")
            .font(
              MacTheme.screenplay(
                PaneLayoutContentMetrics.screenplaySmartTypeFontPointSize, weight: .bold)
            )
            .foregroundStyle(MacTheme.muted)
          ForEach(smartTypeSuggestions, id: \.self) { suggestion in
            Button(suggestion) { requestEdit(kind: smartTypeKind, text: suggestion) }
              .buttonStyle(.plain)
              .font(
                MacTheme.screenplay(
                  PaneLayoutContentMetrics.screenplaySmartTypeFontPointSize, weight: .bold)
              )
              .foregroundStyle(MacTheme.ink)
              .padding(.horizontal, PaneLayoutContentMetrics.screenplayInlineElementMenuInset)
              .frame(height: PaneLayoutContentMetrics.screenplayInlineElementMenuHeight)
              .background(MacTheme.face, in: Capsule())
              .neoRaised(radius: MacTheme.Radius.pill)
              .accessibilityLabel("Use SmartType suggestion \(suggestion)")
          }
        }
      }
      .fixedSize(horizontal: true, vertical: false)
      .padding(.horizontal, PaneLayoutContentMetrics.screenplayInlineElementMenuInset)
      .frame(minWidth: availableWidth, alignment: .leading)
      .hideEnclosingHorizontalScroller()
    }
    .scrollIndicators(.hidden)
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Screenplay inline controls")
  }

  private func screenplayElementMenu() -> some View {
    Button {
      showingElementKindPicker.toggle()
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Text(smartTypeKind.label)
          .lineLimit(1)
        Spacer(minLength: 0)
        Image(systemName: "chevron.down")
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
      .foregroundStyle(MacTheme.ink)
      .padding(.horizontal, PaneLayoutContentMetrics.screenplayInlineElementMenuInset)
      .frame(
        width: PaneLayoutContentMetrics.screenplayInlineElementMenuWidth,
        height: PaneLayoutContentMetrics.screenplayInlineElementMenuHeight
      )
      .background(MacTheme.face, in: Capsule())
      .neoRaised(radius: MacTheme.Radius.pill)
    }
    .buttonStyle(.plain)
    .focusable(false)
    .popover(isPresented: $showingElementKindPicker, arrowEdge: .bottom) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        ForEach(screenplayElementKinds, id: \.self) { kind in
          Button {
            showingElementKindPicker = false
            requestEdit(kind: kind)
          } label: {
            Text(kind.label)
              .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
              .foregroundStyle(MacTheme.ink)
              .frame(
                maxWidth: .infinity,
                minHeight: PaneLayoutContentMetrics.writingOptionsRowHeight,
                alignment: .leading
              )
              .contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .focusable(false)
          .accessibilityIdentifier("writing-screenplay-element-\(kind.rawValue)-\(pane.paneId)")
        }
      }
      .padding(MacTheme.Space.x8)
      .frame(width: PaneLayoutContentMetrics.writingOptionsMenuWidth)
      .background(MacTheme.face)
      .accessibilityElement(children: .contain)
      .accessibilityLabel("Screenplay elements")
    }
    .accessibilityLabel("Screenplay element")
    .accessibilityIdentifier("writing-screenplay-element-\(pane.paneId)")
  }

  private func hoveredVariants(
    in structure: WritingStructureResult
  ) -> WritingLineVariants? {
    guard let hoveredLine else { return nil }
    return ScreenplayInteraction.variants(at: hoveredLine, in: structure)
  }

  private func requestEdit(kind: ScreenplayElementKind, text: String? = nil) {
    guard let file else { return }
    editRequest = ScreenplayEditRequest(
      fileId: file.file.fileId, revision: file.file.revision, kind: kind, text: text)
  }

  private func setFocus(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, focusMode: enabled) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private var documentFileName: String {
    guard let path = writing?.document.file.path else { return pane.name }
    return URL(fileURLWithPath: path).lastPathComponent
  }

  private func sceneCountLabel(_ count: Int) -> String {
    count == 1 ? "1 scene" : "\(count) scenes"
  }

  private var chromeOpacity: Double {
    chromeActivity.visibility == .dimmed
      ? WritingChromeMetrics.standard.dimmedOpacity : 1
  }

  private func updateChrome(_ event: WritingChromeActivityEvent) {
    var next = chromeActivity
    next.reduce(event)
    let duration =
      next.visibility == .dimmed
      ? WritingChromeMetrics.standard.dimDuration
      : WritingChromeMetrics.standard.revealDuration
    withAnimation(.easeOut(duration: duration)) { chromeActivity = next }
  }

  private func requestClose() {
    switch WritingChromePolicy().closeDisposition(
      fileName: documentFileName, dirty: file?.file.dirty == true)
    {
    case .closeImmediately:
      client.closePane(pane.paneId)
    case .confirm:
      showingCloseConfirmation = true
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
    .padding(MacTheme.Space.x12)
    .frame(width: PaneLayoutContentMetrics.screenplaySceneRailWidth)
    .background(MacTheme.face)
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
      .background(
        selected ? MacTheme.selection : Color.clear,
        in: RoundedRectangle(cornerRadius: MacTheme.Radius.control)
      )
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
  let fileId: String
  let revision: String
  let kind: ScreenplayElementKind
  let text: String?

  init(fileId: String, revision: String, kind: ScreenplayElementKind, text: String? = nil) {
    self.fileId = fileId
    self.revision = revision
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
  @Binding var elementMenuAnchor: PaneLayoutPoint?
  @Binding var showingElementMenu: Bool
  @Binding var smartTypeKind: ScreenplayElementKind
  @Binding var smartTypeSuggestions: [String]
  let onEditingActivity: () -> Void

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

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: ScreenplayViewport
    private let textView = ScreenplayTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedRevision: String?
    private var renderedTheme: TerminalColorTheme?
    private var applying = false
    private var appliedRequest: UUID?
    private weak var scrollView: NSScrollView?

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
      NotificationCenter.default.addObserver(
        self, selector: #selector(viewportDidScroll(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      scrollView = scroll
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
      guard !applying, let position = position(textView.selectedRange().location) else { return }
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
      parent.onEditingActivity()
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
      ensurePageLayout()
      textView.needsDisplay = true
      applying = false
      publishElementMenuAnchor(deferred: true)
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
      let raw = source as NSString
      guard selection.location != NSNotFound, selection.location <= raw.length else {
        hideInlineControls()
        return
      }
      let lineRange = raw.lineRange(
        for: NSRange(location: min(selection.location, raw.length), length: 0))
      let contentRange = lineContentRange(lineRange)
      let lineText = raw.substring(with: contentRange)
      let showingElementMenu = WritingChromePolicy().showsScreenplayElementMenu(
        lineText: lineText, selectionLength: selection.length)
      let smartTypeKind = ScreenplayInteraction.suggestionKind(
        at: position(selection.location)?.line ?? parent.activeLine, in: parent.structure)
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
