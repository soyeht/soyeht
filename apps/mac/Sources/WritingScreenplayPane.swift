import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

private enum ScreenplayHeaderPopover: String, Identifiable {
  case options
  case scenes

  var id: String { rawValue }
}

private struct ScreenplayReady {
  let file: EditorFileView
  let document: WritingDocument
  let structure: WritingStructureResult
  let plan: WritingScreenplayLayoutPlan
}

struct WritingScreenplayPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
  @State private var headerPopover: ScreenplayHeaderPopover?
  @State private var chrome = WritingChromeModel()
  @State private var elementMenuAnchor: PaneLayoutPoint?
  @State private var showingElementMenu = false
  @State private var showingElementKindPicker = false
  @State private var smartTypeKind: ScreenplayElementKind = .action
  @State private var smartTypeSuggestions: [String] = []
  @State private var editRequest: ScreenplayEditRequest?
  @State private var hoveredLine: UInt32?
  @State private var hoverY: CGFloat = 0
  @State private var showingBeatBoard = false

  private var context: WritingPaneContext { WritingPaneContext(client: client, pane: pane) }

  var body: some View {
    Group {
      if showingBeatBoard, let writing = context.writing, let structure = writing.structure {
        WritingBeatBoardPane(
          client: client, pane: pane, document: writing.document, structure: structure,
          showScreenplay: { showingBeatBoard = false }
        )
        .paneShell()
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
      } else {
        WritingPaneChrome(
          chrome: chrome, clipsPage: true,
          resolve: { size -> WritingResolvedPane<ScreenplayReady>? in
            guard let writing = context.writing, let file = context.file, file.viewport != nil,
              let structure = writing.structure, let plan = layoutPlan(size: size),
              case .screenplay(let screenplayPlan) = plan.surface
            else { return nil }
            return WritingResolvedPane(
              plan: plan,
              ready: ScreenplayReady(
                file: file, document: writing.document, structure: structure,
                plan: screenplayPlan))
          },
          header: { _ in
            screenplayHeader(
              document: context.writing?.document, structure: context.writing?.structure)
          },
          surface: { _, ready in
            plannedSurface(
              screenplayPlan: ready.plan, file: ready.file, document: ready.document,
              structure: ready.structure)
          }
        )
      }
    }
    .writingPaneLifecycle(context, chrome: chrome)
    .onChange(of: headerPopover != nil) { _, value in
      chrome.send(.menuChanged(value))
    }
    .onChange(of: elementMenuAnchor == nil) { _, hidden in
      if hidden { showingElementKindPicker = false }
    }
  }

  private func layoutPlan(size: CGSize) -> WritingLayoutPlan? {
    MacPaneLayoutMetrics.writingPlan(
      kind: .screenplay, size: size, font: MacPaneLayoutMetrics.screenplayMetric)
  }

  @ViewBuilder private func plannedSurface(
    screenplayPlan: WritingScreenplayLayoutPlan,
    file: EditorFileView, document: WritingDocument, structure: WritingStructureResult
  ) -> some View {
    if let navigationRect = screenplayPlan.navigationRect {
      ScreenplaySceneRail(
        scenes: structure.scenes, elements: structure.elements,
        activeLine: activeLine, select: selectScene
      )
      .writingFrame(navigationRect)
      .accessibilityIdentifier("writing-screenplay-scene-rail-\(pane.paneId)")
      WritingRailDivider(rail: navigationRect)
    }
    screenplayPage(
      file: file, document: document, structure: structure, plan: screenplayPlan
    )
    .writingFrame(screenplayPlan.pageRect)
  }

  private func screenplayHeader(
    document: WritingDocument?, structure: WritingStructureResult?
  ) -> some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: context.documentFileName,
      metadata: structure.map { WritingDisplayText.count($0.scenes.count, kind: .scene) },
      companionLinked: companionLinked, opacity: chrome.opacity,
      headerDrag: headerDrag,
      onHover: { chrome.send(.headerHoverChanged($0)) },
      trailing: {
        WritingOptionsButton(paneId: pane.paneId, name: "Screenplay") {
          headerPopover = headerPopover == nil ? .options : nil
        }
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

  private func screenplayOptions(document: WritingDocument) -> some View {
    WritingOptionsMenu(
      name: "Screenplay", paneId: pane.paneId,
      groups: MacPaneLayoutMetrics.chromePolicy.plan(
        kind: .screenplay, fullWidth: document.fullWidth
      ).menuGroups,
      connected: client.connected, layoutActionsDisabled: layoutActionsDisabled,
      dismiss: { headerPopover = nil }, onSplit: onSplit,
      onClose: { chrome.requestClose(context) },
      specific: { action in
        switch action {
        case .ideaBoard:
          WritingOptionButton(action, paneId: pane.paneId) {
            headerPopover = nil
            showingBeatBoard = true
          }
        case .scenes:
          WritingOptionButton(action, paneId: pane.paneId) {
            headerPopover = .scenes
          }
        case .focusMode:
          WritingOptionButton(
            action, paneId: pane.paneId, title: action.title(focusMode: document.focusMode),
            active: document.focusMode, disabled: !client.connected
          ) {
            headerPopover = nil
            context.setPreferences(focusMode: !document.focusMode)
          }
        default:
          EmptyView()
        }
      }
    )
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
        onEditingActivity: { chrome.send(.edited) })
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
        Text(WritingDisplayText.screenplayElementName(smartTypeKind))
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
        ForEach(WritingDisplayText.screenplayElementKinds, id: \.self) { kind in
          Button {
            showingElementKindPicker = false
            requestEdit(kind: kind)
          } label: {
            Text(WritingDisplayText.screenplayElementName(kind))
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
    guard let file = context.file else { return }
    editRequest = ScreenplayEditRequest(
      fileId: file.file.fileId, revision: file.file.revision, kind: kind, text: text)
  }

  private func selectScene(_ scene: ScreenplayScene) {
    targetLine = scene.line
    context.revealLine(scene.line)
  }
}

private struct ScreenplaySceneRail: View {
  let scenes: [ScreenplayScene]
  let elements: [ScreenplayElement]
  let activeLine: UInt32
  let select: (ScreenplayScene) -> Void

  var body: some View {
    WritingRail(
      title: "SCENES", count: scenes.count, titleSize: MacTheme.TypeScale.small, titleTracking: 1,
      width: PaneLayoutContentMetrics.screenplaySceneRailWidth, itemSpacing: MacTheme.Space.x7
    ) {
      ForEach(scenes, id: \.sceneId) { scene in
        sceneButton(scene)
      }
    }
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
    client.perform {
      _ = try await client.selectWritingVariant(
        fileId: fileId, lineId: variants.lineId, variantId: variant.variantId)
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
    client.perform {
      _ = try await client.requestWritingVariant(
        fileId: fileId, lineId: variants.lineId, agentPaneId: agent.paneId,
        instruction: "Write one alternate take for this screenplay line.")
    }
  }

  private func removeSelected() {
    guard variants.alternates.count > 1, let selected else { return }
    client.perform {
      _ = try await client.removeWritingVariant(
        fileId: fileId, lineId: variants.lineId, variantId: selected.variantId)
    }
  }
}

struct ScreenplayEditRequest: Equatable {
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
