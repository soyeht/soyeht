import SoyehtClient
import SoyehtProtocol
import SwiftUI

@MainActor
struct PhoneWritingLeaf: View {
  @Bindable var client: EngineClient
  let leaf: PhoneWritingLeafLayout
  let pane: Pane?
  let scrollActivityTick: Int
  let onAction: (PhoneWritingStateAction) -> Void
  let onSplit: (PaneLayoutAxis) -> Void

  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @Environment(\.scenePhase) private var scenePhase
  @State private var chromeActivity = WritingChromeActivityState()
  @State private var showingCloseConfirmation = false
  @State private var showingIdeaBoard = false
  @State private var showingScenes = false
  @State private var presentingSlides = false
  @State private var selectedPresentationSlide: UInt32 = 0

  private var writing: WritingDocumentView? {
    guard let fileId = leaf.identity.fileId else { return nil }
    return client.writingDocuments[fileId]
  }

  private var file: EditorFileView? {
    guard let fileId = leaf.identity.fileId else { return nil }
    return client.editorFiles[fileId]
  }

  var body: some View {
    VStack(spacing: 0) {
      header
        .frame(width: leaf.headerRect.size.width, height: leaf.headerRect.size.height)
      if leaf.expanded, let content = leaf.contentRect {
        contentView
          .frame(width: content.size.width, height: content.size.height)
      }
    }
    .frame(width: leaf.rowRect.size.width, height: leaf.rowRect.size.height, alignment: .top)
    .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: DesignTokens.Radius.panel))
    .accessibilityElement(children: .contain)
    .confirmationDialog(
      closeTitle, isPresented: $showingCloseConfirmation, titleVisibility: .visible
    ) {
      Button("Close pane", role: .destructive) { closePane() }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text("Unsaved changes will be lost.")
    }
    .sheet(isPresented: $showingIdeaBoard) {
      if let writing, let structure = writing.structure {
        PhoneWritingIdeaBoard(
          client: client, writing: writing, structure: structure,
          accessibilityName: leaf.name)
      }
    }
    .sheet(isPresented: $showingScenes) {
      if let writing, let file, let structure = writing.structure {
        PhoneWritingScenes(
          client: client, writing: writing, file: file, structure: structure,
          accessibilityName: leaf.name)
      }
    }
    .fullScreenCover(isPresented: $presentingSlides) {
      if let writing {
        PhoneWritingPresentationPlayer(
          client: client, writing: writing,
          slides: writing.structure?.slides ?? [],
          initialSlide: selectedPresentationSlide,
          accessibilityName: leaf.name)
      } else {
        ContentUnavailableView(
          "Presentation unavailable", systemImage: "rectangle.on.rectangle")
      }
    }
    .onChange(of: scrollActivityTick) { _, _ in updateChrome(.pointerMoved) }
    .onChange(of: scenePhase) { _, phase in
      if phase == .active { updateChrome(.pointerMoved) }
    }
    .task(
      id: leaf.expanded
        ? "\(leaf.identity.paneId):\(writing?.loadIdentity ?? "missing")" : nil
    ) {
      guard leaf.expanded else { return }
      do { try await client.loadWritingPane(leaf.identity.paneId) } catch {
        client.problem = "Could not load the writing document."
      }
    }
  }

  private var header: some View {
    VStack(spacing: 0) {
      HStack(spacing: 0) {
        Button {
          toggle()
        } label: {
          HStack(spacing: 8) {
            Image(systemName: icon)
              .font(.system(size: PhoneWritingLayoutMetrics.standard.headerKindIconSize))
              .foregroundStyle(PhonePalette.secondary)
              .frame(width: 22)
            Text(documentFileName)
              .font(
                .custom(
                  DesignTokens.Fonts.chrome,
                  size: PhoneWritingLayoutMetrics.standard.headerNamePointSize,
                  relativeTo: .subheadline
                ).weight(.semibold)
              )
              .foregroundStyle(PhonePalette.secondary)
              .lineLimit(1)
              .truncationMode(.middle)
              .frame(maxWidth: .infinity, alignment: .leading)
          }
          .frame(
            maxWidth: .infinity,
            minHeight: chromePlan.headerHeight,
            maxHeight: chromePlan.headerHeight,
            alignment: .leading
          )
          .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(accessibilityLabel)
        .accessibilityValue(leaf.expanded ? "Expanded" : "Collapsed")
        .accessibilityHint(leaf.expanded ? "Collapses this document" : "Opens this document")
        .accessibilityIdentifier("phone-writing-header-\(leaf.identity.paneId)")

        if let metadataLabel {
          Text(metadataLabel)
            .font(
              .custom(
                DesignTokens.Fonts.chrome,
                size: PhoneWritingLayoutMetrics.standard.headerCountPointSize,
                relativeTo: .subheadline)
            )
            .foregroundStyle(PhonePalette.muted)
            .lineLimit(1)
            .truncationMode(.tail)
            .layoutPriority(1)
            .accessibilityLabel(metadataLabel)
        }
        if leaf.companionAgentPaneId != nil {
          Image(systemName: "link")
            .font(.system(size: DesignTokens.TypeScale.body))
            .foregroundStyle(PhonePalette.secondary)
            .frame(width: 30, height: chromePlan.minimumActionTarget)
            .accessibilityLabel("Linked agent")
        }
        writingOptions
        Button {
          toggle()
        } label: {
          Image(systemName: leaf.expanded ? "chevron.up" : "chevron.down")
            .font(.system(size: PhoneWritingLayoutMetrics.standard.headerActionIconSize))
            .foregroundStyle(PhonePalette.secondary)
            .frame(
              width: chromePlan.minimumActionTarget,
              height: chromePlan.minimumActionTarget
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(leaf.expanded ? "Collapse document" : "Expand document")
      }
      .padding(.leading, PhoneWritingLayoutMetrics.standard.headerHorizontalInset)
      .frame(
        maxWidth: .infinity,
        minHeight: chromePlan.headerHeight,
        maxHeight: chromePlan.headerHeight
      )
      .opacity(chromeOpacity)

      if leaf.companionAgentPaneId != nil {
        HStack(spacing: 0) {
          tabButton(.document, title: "Document", systemImage: icon)
          tabButton(.agent, title: "Agent", systemImage: "bubble.left.and.bubble.right")
        }
        .frame(height: chromePlan.headerHeight)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Linked document and agent tabs")
      }
    }
  }

  private var writingOptions: some View {
    Menu {
      ForEach(Array(chromePlan.menuGroups.enumerated()), id: \.offset) { index, group in
        if index > 0 { Divider() }
        ForEach(group, id: \.rawValue) { action in
          writingMenuAction(action)
        }
      }
    } label: {
      Image(systemName: "ellipsis")
        .font(.system(size: PhoneWritingLayoutMetrics.standard.headerActionIconSize))
        .foregroundStyle(PhonePalette.secondary)
        .frame(
          width: chromePlan.minimumActionTarget,
          height: chromePlan.minimumActionTarget
        )
        .contentShape(Rectangle())
    }
    .accessibilityLabel("Writing options")
    .accessibilityIdentifier("phone-writing-options-\(leaf.identity.paneId)")
    // SwiftUI Menu has no reliable close callback. Revealing without latching a
    // synthetic "open" state keeps the next edit free to dim the chrome.
    .simultaneousGesture(TapGesture().onEnded { updateChrome(.pointerMoved) })
  }

  private var chromePlan: PhoneWritingChromePlan {
    PhoneWritingMetrics.chromePolicy.plan(
      kind: leaf.kind, fullWidth: writing?.document.fullWidth == true)
  }

  @ViewBuilder private func writingMenuAction(_ action: WritingChromeAction) -> some View {
    switch action {
    case .font:
      Menu("Font") {
        fontButton(WritingDisplayText.fontName(.reading_serif), .reading_serif)
        fontButton(WritingDisplayText.fontName(.classic_serif), .classic_serif)
        fontButton(WritingDisplayText.fontName(.sans), .sans)
      }
    case .useFullWidth:
      Button("Use full width") { setFullWidth(true) }
    case .useReadingWidth:
      Button("Use reading width") { setFullWidth(false) }
    case .focusMode:
      focusButton
    case .ideaBoard:
      Button("Idea board", systemImage: "rectangle.3.group") {
        openScreenplaySheet(.ideaBoard)
      }
    case .scenes:
      Button("Scenes", systemImage: "list.number") {
        openScreenplaySheet(.scenes)
      }
    case .present:
      Button("Present", systemImage: "play.rectangle") { presentSlides() }
    case .splitColumns:
      Button("Split columns", systemImage: "rectangle.split.2x1") {
        updateChrome(.menuChanged(false))
        onSplit(.horizontal)
      }
    case .splitRows:
      Button("Split rows", systemImage: "rectangle.split.1x2") {
        updateChrome(.menuChanged(false))
        onSplit(.vertical)
      }
    case .closePane:
      Button("Close pane", systemImage: "xmark", role: .destructive) { requestClose() }
    case .bold, .italic, .heading, .list, .quote, .link, .image:
      EmptyView()
    }
  }

  private var focusButton: some View {
    Button(writing?.document.focusMode == true ? "Leave Focus" : "Focus") {
      setFocus(writing?.document.focusMode != true)
    }
  }

  @ViewBuilder private var contentView: some View {
    if leaf.activeTab == .agent, let agentPaneId = leaf.companionAgentPaneId {
      AgentConversationView(
        client: client, paneId: agentPaneId, showsConfiguration: false,
        onExplicitRemoteAction: {
          PhoneWritingFocusPolicy.apply(.sendToAgent(agentPaneId)) { client.select($0) }
        }
      )
      .accessibilityIdentifier("phone-writing-agent-\(agentPaneId)")
      .task(id: agentPaneId) {
        do { try await client.loadAgentConversation(agentPaneId) } catch {
          client.problem = "Could not load the linked agent."
        }
      }
    } else {
      documentContent
    }
  }

  @ViewBuilder private var documentContent: some View {
    switch leaf.availability {
    case .missingDocument:
      writingState("Loading document", systemImage: "arrow.trianglehead.2.clockwise")
    case .inconsistentDocument:
      writingState("This document needs to reload", systemImage: "exclamationmark.triangle")
    case .ready:
      if leaf.kind == .text, let writing, let content = leaf.contentRect {
        PhoneWritingTextSurface(
          client: client, pane: pane, writing: writing, file: file,
          size: content.size, font: leaf.font ?? PhoneWritingMetrics.fonts.prose,
          accessibilityName: leaf.name,
          onEditingActivity: { updateChrome(.edited) },
          onSelectionActivity: { updateChrome(.selectionChanged($0)) },
          onScrollActivity: { updateChrome(.pointerMoved) })
      } else if leaf.kind == .screenplay {
        if let writing, let file, let structure = writing.structure,
          let content = leaf.contentRect, let pageSize = leaf.screenplayPageSize,
          let fitScale = leaf.screenplayFitScale
        {
          PhoneWritingScreenplaySurface(
            client: client, identity: leaf.identity, writing: writing, file: file,
            structure: structure, viewportSize: content.size, pageSize: pageSize,
            fitScale: fitScale, zoomMultiplier: leaf.screenplayZoom,
            accessibilityName: leaf.name,
            onEditingActivity: { updateChrome(.edited) },
            onSelectionActivity: { updateChrome(.selectionChanged($0)) },
            onScrollActivity: { updateChrome(.pointerMoved) },
            onZoomChanged: { zoom in
              onAction(.setScreenplayZoom(leaf.identity, zoom))
            })
        } else {
          writingState("Loading screenplay", systemImage: "arrow.trianglehead.2.clockwise")
        }
      } else if leaf.kind == .presentation {
        if let writing, let file, let structure = writing.structure,
          let content = leaf.contentRect
        {
          PhoneWritingPresentationSurface(
            client: client, writing: writing, file: file, structure: structure,
            size: content.size, font: leaf.font ?? PhoneWritingMetrics.fonts.prose,
            accessibilityName: leaf.name,
            selectedSlide: $selectedPresentationSlide,
            onActivity: { updateChrome(.pointerMoved) })
        } else {
          writingState("Loading presentation", systemImage: "arrow.trianglehead.2.clockwise")
        }
      } else {
        writingState("Open this document on the Mac", systemImage: icon)
      }
    }
  }

  private func writingState(_ title: String, systemImage: String) -> some View {
    ContentUnavailableView(title, systemImage: systemImage)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var icon: String {
    switch leaf.kind {
    case .text: "doc.richtext"
    case .screenplay: "text.alignleft"
    case .presentation: "rectangle.on.rectangle"
    default: "doc"
    }
  }

  private var kindName: String {
    switch leaf.kind {
    case .text: "Text"
    case .screenplay: "Screenplay"
    case .presentation: "Presentation"
    default: "Writing document"
    }
  }

  private var accessibilityLabel: String {
    let link = leaf.companionAgentPaneId == nil ? "" : ", linked agent"
    return "\(pane?.name ?? leaf.name), \(kindName)\(link)"
  }

  private var documentFileName: String {
    guard let path = writing?.document.file.path else { return pane?.name ?? leaf.name }
    return URL(fileURLWithPath: path).lastPathComponent
  }

  private var metadataLabel: String? {
    guard let structure = writing?.structure else { return nil }
    switch leaf.kind {
    case .text:
      return WritingDisplayText.count(Int(structure.wordCount), kind: .word)
    case .screenplay:
      return WritingDisplayText.count(structure.scenes.count, kind: .scene)
    case .presentation:
      return WritingDisplayText.count(structure.slides.count, kind: .slide)
    default:
      return nil
    }
  }

  private var chromeOpacity: Double {
    chromeActivity.visibility == .dimmed
      ? WritingChromeMetrics.standard.dimmedOpacity : 1
  }

  private var closeTitle: String { "Close \(documentFileName)?" }

  private func updateChrome(_ event: WritingChromeActivityEvent) {
    let before = chromeActivity.visibility
    var updated = chromeActivity
    updated.reduce(event)
    let after = updated.visibility
    let transition: PhoneWritingChromeTransition = after == .dimmed ? .dim : .reveal
    let duration = PhoneWritingMetrics.chromePolicy.duration(
      for: transition, reduceMotion: reduceMotion)
    if before == after || duration == 0 {
      chromeActivity = updated
    } else {
      withAnimation(.easeInOut(duration: duration)) { chromeActivity = updated }
    }
  }

  private func fontButton(_ title: String, _ font: WritingFont) -> some View {
    Button {
      setFont(font)
    } label: {
      if writing?.document.font == font {
        Label(title, systemImage: "checkmark")
      } else {
        Text(title)
      }
    }
  }

  private func setFont(_ font: WritingFont) {
    guard let fileId = writing?.document.file.fileId else { return }
    updateChrome(.menuChanged(false))
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, font: font) } catch {
        client.problem = "Could not change the document font."
      }
    }
  }

  private func setFullWidth(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    updateChrome(.menuChanged(false))
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, fullWidth: enabled) } catch {
        client.problem = "Could not change the document width."
      }
    }
  }

  private func setFocus(_ enabled: Bool) {
    guard let fileId = writing?.document.file.fileId else { return }
    updateChrome(.menuChanged(false))
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, focusMode: enabled) } catch {
        client.problem = "Could not change Focus."
      }
    }
  }

  private enum ScreenplaySheet {
    case ideaBoard
    case scenes
  }

  private func openScreenplaySheet(_ sheet: ScreenplaySheet) {
    updateChrome(.menuChanged(false))
    Task { @MainActor in
      do {
        try await client.loadWritingPane(leaf.identity.paneId)
        switch sheet {
        case .ideaBoard: showingIdeaBoard = true
        case .scenes: showingScenes = true
        }
      } catch {
        client.problem = "Could not load the screenplay."
      }
    }
  }

  private func presentSlides() {
    updateChrome(.menuChanged(false))
    Task { @MainActor in
      do {
        try await client.loadWritingPane(leaf.identity.paneId)
        presentingSlides = true
      } catch {
        client.problem = "Could not load the presentation."
      }
    }
  }

  private func requestClose() {
    updateChrome(.menuChanged(false))
    let dirty = file?.file.dirty == true
    switch PhoneWritingMetrics.chromePolicy.writingChrome.closeDisposition(
      fileName: documentFileName, dirty: dirty)
    {
    case .closeImmediately: closePane()
    case .confirm: showingCloseConfirmation = true
    }
  }

  private func closePane() {
    Task { @MainActor in await client.closePaneSavingEdits(leaf.identity.paneId) }
  }

  private func tabButton(
    _ tab: PhoneWritingTab, title: String, systemImage: String
  ) -> some View {
    let selected = leaf.activeTab == tab
    return Button {
      onAction(.selectTab(leaf.identity, tab))
    } label: {
      Label(title, systemImage: systemImage)
        .font(.system(.subheadline, design: .rounded, weight: .semibold))
        .lineLimit(1)
        .frame(
          maxWidth: .infinity,
          minHeight: chromePlan.minimumActionTarget,
          maxHeight: chromePlan.minimumActionTarget
        )
        .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .foregroundStyle(selected ? PhonePalette.accent : PhonePalette.secondary)
    .background(selected ? PhonePalette.accent.opacity(0.12) : Color.clear)
    .accessibilityLabel("\(title) tab")
    .accessibilityValue(selected ? "Selected" : "Not selected")
    .accessibilityAddTraits(selected ? .isSelected : [])
    .accessibilityIdentifier(
      "phone-writing-tab-\(tab.rawValue)-\(leaf.identity.paneId)")
  }

  private func toggle() {
    updateChrome(.pointerMoved)
    Task { @MainActor in
      if leaf.expanded, let fileId = leaf.identity.fileId {
        await client.flushEditorEdits(fileId: fileId)
      }
      onAction(.toggle(leaf.identity))
    }
  }
}
