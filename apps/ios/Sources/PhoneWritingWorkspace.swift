import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

private enum PhoneWritingSplitChoice: String, CaseIterable, Identifiable {
  case text
  case screenplay
  case presentation
  case agent

  var id: String { rawValue }
  var title: String {
    switch self {
    case .text: "Text"
    case .screenplay: "Screenplay"
    case .presentation: "Presentation"
    case .agent: "Agent"
    }
  }
  var kind: PaneLayoutKind {
    switch self {
    case .text: .text
    case .screenplay: .screenplay
    case .presentation: .presentation
    case .agent: .agent
    }
  }
  var section: WritingSection? {
    switch self {
    case .text: .texts
    case .screenplay: .screenplays
    case .presentation: .presentations
    case .agent: nil
    }
  }
}

private struct PhoneWritingSplitRequest: Identifiable {
  let id = UUID()
  let anchorPaneId: String
  let anchorKind: PaneLayoutKind
  let axis: PaneLayoutAxis
  let baseTree: PaneLayoutTree
}

@MainActor
struct PhoneWritingWorkspace: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let initialPaneId: String

  @State private var localState: PhoneWritingLocalState?
  @State private var showingNewText = false
  @State private var newTextTitle = ""
  @State private var creatingText = false
  @State private var splitRequest: PhoneWritingSplitRequest?
  @State private var splitChoice: PhoneWritingSplitChoice = .text
  @State private var splitTitle = ""
  @State private var splitHarnessId = ""
  @State private var creatingSplit = false
  @State private var scrollActivityTick = 0
  private let stateStore: PhoneWritingLocalStateStore
  private let engineId: String

  init(
    client: EngineClient, workspaceId: String, initialPaneId: String,
    stateStore: PhoneWritingLocalStateStore = PhoneWritingLocalStateStore()
  ) {
    self.client = client
    self.workspaceId = workspaceId
    self.initialPaneId = initialPaneId
    self.stateStore = stateStore
    engineId = client.engineId ?? "unpaired"
    _localState = State(
      initialValue: stateStore.load(
        engineId: client.engineId ?? "unpaired", workspaceId: workspaceId))
  }

  var body: some View {
    GeometryReader { proxy in
      let plan = layoutPlan(viewport: proxy.size)
      Group {
        if plan.leaves.isEmpty {
          ContentUnavailableView(
            "No writing documents", systemImage: "text.book.closed",
            description: Text("Create a text document or open one on the Mac."))
        } else {
          ScrollView(.vertical) {
            ZStack(alignment: .topLeading) {
              Color.clear
                .frame(width: plan.contentSize.width, height: plan.contentSize.height)
                .allowsHitTesting(false)
              ForEach(plan.leaves, id: \.identity) { leaf in
                PhoneWritingLeaf(
                  client: client, leaf: leaf,
                  pane: pane(withId: leaf.identity.paneId),
                  scrollActivityTick: scrollActivityTick,
                  onAction: { action in apply(action, to: plan) },
                  onSplit: { axis in beginSplit(leaf: leaf, axis: axis) }
                )
                .phoneWritingFrame(leaf.rowRect)
              }
            }
          }
          .scrollBounceBehavior(.basedOnSize)
          .simultaneousGesture(
            DragGesture(minimumDistance: 3).onChanged { _ in
              scrollActivityTick &+= 1
            })
        }
      }
      .background(PhonePalette.canvas)
      .onAppear {
        reconcile(plan.localState)
      }
      .onChange(of: plan.localState) { _, reconciled in reconcile(reconciled) }
    }
    .navigationTitle(workspaceName)
    .navigationBarTitleDisplayMode(.inline)
    .toolbar {
      ToolbarItem(placement: .topBarTrailing) {
        Button {
          newTextTitle = ""
          showingNewText = true
        } label: {
          Image(systemName: "plus")
        }
        .accessibilityLabel("New text document")
        .accessibilityIdentifier("phone-writing-new-text")
      }
    }
    .sheet(isPresented: $showingNewText) { newTextSheet }
    .sheet(item: $splitRequest) { request in splitSheet(request) }
  }

  private var newTextSheet: some View {
    NavigationStack {
      Form {
        TextField("Title", text: $newTextTitle)
          .textInputAutocapitalization(.sentences)
          .accessibilityIdentifier("phone-writing-new-text-title")
      }
      .navigationTitle("New text")
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { showingNewText = false }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Create") { createText() }
            .disabled(
              newTextTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                || creatingText
            )
            .accessibilityIdentifier("phone-writing-create-text")
        }
      }
    }
    .presentationDetents([.medium])
  }

  private func splitSheet(_ request: PhoneWritingSplitRequest) -> some View {
    NavigationStack {
      Form {
        Picker("Pane", selection: $splitChoice) {
          ForEach(availableSplitChoices(for: request)) { choice in
            Text(choice.title).tag(choice)
          }
        }
        .phoneWritingSheetRow()
        if splitChoice == .agent {
          Picker("Harness", selection: $splitHarnessId) {
            ForEach(availableHarnesses, id: \.id) { harness in
              Text(harness.name).tag(harness.id)
            }
          }
          .phoneWritingSheetRow()
          if availableHarnesses.isEmpty {
            Text("No ready agent harness is available on this Mac.")
              .foregroundStyle(PhonePalette.secondary)
              .phoneWritingSheetRow()
          }
        } else {
          TextField(
            "", text: $splitTitle,
            prompt: Text("Title").foregroundStyle(PhonePalette.secondary)
          )
          .textInputAutocapitalization(.sentences)
          .accessibilityIdentifier("phone-writing-split-title")
          .phoneWritingSheetRow()
        }
      }
      .phoneWritingSheetScroll()
      .phoneWritingSheetNavigation(
        request.axis == .horizontal ? "Split columns" : "Split rows"
      )
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Cancel") { splitRequest = nil }
        }
        ToolbarItem(placement: .confirmationAction) {
          Button("Create") { createSplit(request) }
            .disabled(!splitIsReady || creatingSplit)
            .accessibilityIdentifier("phone-writing-create-split")
        }
      }
    }
    .phoneWritingSheet(.split)
    .onChange(of: availableHarnesses.map(\.id)) { _, harnessIds in
      guard splitChoice == .agent, !harnessIds.contains(splitHarnessId) else { return }
      splitHarnessId =
        HarnessPickerModel.defaultHarness(
          in: availableHarnesses, preferredId: client.preferredHarnessId)?.id ?? ""
    }
  }

  private var availableHarnesses: [HarnessInfo] {
    HarnessPickerModel.sections(client.harnesses)
      .flatMap(\.items)
      .filter(\.available)
  }

  private func availableSplitChoices(
    for request: PhoneWritingSplitRequest
  ) -> [PhoneWritingSplitChoice] {
    let allowed = PhoneWritingMetrics.chromePolicy.plan(kind: request.anchorKind).splitKinds
    return PhoneWritingSplitChoice.allCases.filter { allowed.contains($0.kind) }
  }

  private var splitIsReady: Bool {
    if splitChoice == .agent { return availableHarnesses.contains { $0.id == splitHarnessId } }
    return !splitTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  private var workspaceName: String {
    client.workspaces.first { $0.workspaceId == workspaceId }?.name ?? "Writing"
  }

  private func pane(withId paneId: String) -> Pane? {
    client.panes.first { $0.paneId == paneId }
  }

  private func layoutPlan(viewport: CGSize) -> PhoneWritingLayoutPlan {
    let panes = client.panes.filter { $0.workspaceId == workspaceId }
    let paneNames = Dictionary(uniqueKeysWithValues: panes.map { ($0.paneId, $0.name) })
    let documents = client.writingDocuments.values.compactMap {
      view -> PhoneWritingDocumentDescriptor? in
      let document = view.document
      guard document.file.workspaceId == workspaceId,
        let pane = panes.first(where: { $0.paneId == document.file.paneId })
      else { return nil }
      let kind = PaneLayoutKind(mode: pane.mode, writingKind: document.kind)
      return PhoneWritingDocumentDescriptor(
        paneId: pane.paneId, fileId: document.file.fileId, name: pane.name, kind: kind,
        font: PhoneWritingMetrics.metric(for: document), fullWidth: document.fullWidth)
    }
    return PhoneWritingMetrics.policy.layout(
      PhoneWritingLayoutRequest(
        tree: layoutTree(livePanes: panes), documents: documents, paneNames: paneNames,
        // The pane used to enter this phone screen seeds device-local state without
        // mutating the engine's shared focus.
        selectedPaneId: initialPaneId, localState: localState,
        snapshotIsAuthoritative: client.connected,
        viewport: PaneLayoutSize(width: viewport.width, height: viewport.height)))
  }

  private func layoutTree(livePanes: [Pane]) -> PaneLayoutTree? {
    client.presentedLayoutTree(
      workspaceId: workspaceId, livePanes: livePanes, sizing: PhoneWritingMetrics.sizing)
  }

  private func apply(_ action: PhoneWritingStateAction, to plan: PhoneWritingLayoutPlan) {
    let updated = PhoneWritingMetrics.policy.applying(action, to: plan)
    localState = updated
    persist(updated)
  }

  private func reconcile(_ state: PhoneWritingLocalState) {
    // A paired-device disconnect clears replicas by design. Do not mistake that transient
    // empty snapshot for panes being closed and overwrite the last local presentation.
    guard client.connected else { return }
    guard localState != state else { return }
    localState = state
    persist(state)
  }

  private func persist(_ state: PhoneWritingLocalState) {
    do {
      try stateStore.save(state, engineId: engineId, workspaceId: workspaceId)
    } catch {
      client.problem = "Could not remember the iPhone writing view."
    }
  }

  private func beginSplit(leaf: PhoneWritingLeafLayout, axis: PaneLayoutAxis) {
    let panes = client.panes.filter { $0.workspaceId == workspaceId }
    guard let tree = layoutTree(livePanes: panes) else {
      client.problem = "Could not open the split chooser. The layout was not changed."
      return
    }
    splitChoice = .text
    splitTitle = ""
    client.refreshHarnesses()
    splitHarnessId =
      HarnessPickerModel.defaultHarness(
        in: client.harnesses, preferredId: client.preferredHarnessId)?.id ?? ""
    splitRequest = PhoneWritingSplitRequest(
      anchorPaneId: leaf.identity.paneId, anchorKind: leaf.kind, axis: axis, baseTree: tree)
  }

  private func createSplit(_ request: PhoneWritingSplitRequest) {
    guard splitIsReady, !creatingSplit else { return }
    let choice = splitChoice
    let title = splitTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    let harnessId = splitHarnessId
    creatingSplit = true
    Task { @MainActor in
      defer { creatingSplit = false }
      do {
        let insertion = PaneLayoutProvisionalInsertion(
          baseTree: request.baseTree,
          placeholder: PaneLayoutLeaf(paneId: "phone-new-pane", kind: choice.kind),
          placement: .split(anchorPaneId: request.anchorPaneId, axis: request.axis))
        let paneId: String
        let completed: PaneLayoutTree
        if let section = choice.section {
          let result = try await client.createWritingDocument(
            workspaceId: workspaceId, section: section, title: title,
            selectPane: false)
          paneId = result.pane.paneId
          completed = try insertion.completedTree(
            with: PaneLayoutLeaf(paneId: paneId, kind: choice.kind),
            sizing: PhoneWritingMetrics.sizing)
          let identity = PhoneWritingIdentity(
            paneId: paneId, fileId: result.document.file.fileId)
          let state = (localState ?? PhoneWritingLocalState()).opening(identity)
          localState = state
          persist(state)
        } else {
          guard let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
          else { throw PhoneWritingSplitFailure.missingWorkspace }
          paneId = try await client.createAgentPaneNow(
            directory: URL(fileURLWithPath: workspace.path), harness: harnessId,
            model: nil, effort: nil, selectPane: false)
          completed = try insertion.completedWritingCompanionTree(
            with: PaneLayoutLeaf(paneId: paneId, kind: .agent),
            writingPaneId: request.anchorPaneId, sizing: PhoneWritingMetrics.sizing)
        }
        try client.paneLayouts.commit(tree: completed, workspaceId: workspaceId)
        splitRequest = nil
      } catch {
        client.problem = "Could not create the split. The layout was not changed."
      }
    }
  }

  private func createText() {
    let title = newTextTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !title.isEmpty, !creatingText else { return }
    creatingText = true
    Task { @MainActor in
      defer { creatingText = false }
      do {
        let result = try await client.createWritingDocument(
          workspaceId: workspaceId, section: .texts, title: title, selectPane: false)
        let identity = PhoneWritingIdentity(
          paneId: result.pane.paneId, fileId: result.document.file.fileId)
        let state = (localState ?? PhoneWritingLocalState()).opening(identity)
        localState = state
        persist(state)
        showingNewText = false
      } catch {
        client.problem = "Could not create the text document."
      }
    }
  }
}

private enum PhoneWritingSplitFailure: Error {
  case missingWorkspace
}

@MainActor
enum PhoneWritingMetrics {
  static let screenplayFontName = "Courier Prime"

  static let screenplayMetric: PaneLayoutFontMetric = {
    let font = screenplayFont()
    let width = ("0" as NSString).size(withAttributes: [.font: font]).width
    return PaneLayoutFontMetric(
      characterWidth: ceil(width * 1_000) / 1_000,
      lineHeight: PaneLayoutContentMetrics.screenplayLineHeight,
      pointSize: PaneLayoutContentMetrics.screenplayFontPointSize)
  }()

  static let fonts = PaneLayoutFontMetrics(
    terminal: PaneLayoutFontMetric(characterWidth: 8, lineHeight: 18, pointSize: 13),
    editor: PaneLayoutFontMetric(characterWidth: 6.9, lineHeight: 19, pointSize: 11.5),
    chrome: PaneLayoutFontMetric(characterWidth: 7.2, lineHeight: 17, pointSize: 12),
    agent: PaneLayoutFontMetric(characterWidth: 9, lineHeight: 21, pointSize: 15),
    prose: metric(for: .reading_serif),
    screenplay: screenplayMetric)
  static let floors = PaneLayoutFloorFormula.surfaceFloors(fonts: fonts)
  static let sizing = PaneLayoutSizingPolicy(
    floors: floors, gaps: PaneLayoutSpacing.gaps)
  static let policy = PhoneWritingLayoutPolicy(floors: floors)
  static let chromePolicy = PhoneWritingChromePolicy()
  static let sheetPolicy = PhoneWritingSheetPolicy()
  static let bodyWritingMetrics = PhoneWritingLayoutMetrics.standard.bodyWritingMetrics()
  static let bodyFloors = PhoneWritingLayoutMetrics.standard.bodySurfaceFloors(from: floors)

  static func metric(for document: WritingDocument) -> PaneLayoutFontMetric {
    document.kind == .screenplay ? fonts.screenplay : metric(for: document.font)
  }

  static func metric(for font: WritingFont) -> PaneLayoutFontMetric {
    let uiFont = uiFont(for: font)
    let width = ("0" as NSString).size(withAttributes: [.font: uiFont]).width
    return PaneLayoutFontMetric(
      characterWidth: ceil(width * 1_000) / 1_000,
      lineHeight: PaneLayoutContentMetrics.textLineHeight,
      pointSize: PaneLayoutContentMetrics.textFontPointSize)
  }

  static func uiFont(for font: WritingFont) -> UIFont {
    let size = PaneLayoutContentMetrics.textFontPointSize
    switch font {
    case .reading_serif:
      return UIFont(name: "Newsreader", size: size)
        ?? UIFont(name: "Newsreader-Regular", size: size)
        ?? UIFont.systemFont(ofSize: size)
    case .classic_serif:
      return UIFont(name: "TimesNewRomanPSMT", size: size)
        ?? UIFont.systemFont(ofSize: size)
    case .sans:
      return UIFont(name: "Nunito-Regular", size: size)
        ?? UIFont(name: "Nunito", size: size)
        ?? UIFont.systemFont(ofSize: size)
    }
  }

  static func screenplayFont(bold: Bool = false) -> UIFont {
    let size = PaneLayoutContentMetrics.screenplayFontPointSize
    if bold {
      return UIFont(name: "CourierPrime-Bold", size: size)
        ?? UIFont(name: "Courier Prime Bold", size: size)
        ?? UIFont.monospacedSystemFont(ofSize: size, weight: .bold)
    }
    return UIFont(name: screenplayFontName, size: size)
      ?? UIFont(name: "CourierPrime-Regular", size: size)
      ?? UIFont.monospacedSystemFont(ofSize: size, weight: .regular)
  }
}

@MainActor
private struct PhoneWritingLeaf: View {
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
    .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 18))
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
                  "Nunito", size: PhoneWritingLayoutMetrics.standard.headerNamePointSize,
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
                "Nunito", size: PhoneWritingLayoutMetrics.standard.headerCountPointSize,
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
            .font(.system(size: 15))
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
    Task { @MainActor in
      if let fileId = leaf.identity.fileId { await client.flushEditorEdits(fileId: fileId) }
      client.closePane(leaf.identity.paneId)
    }
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

@MainActor
private struct PhoneWritingTextSurface: View {
  @Bindable var client: EngineClient
  let pane: Pane?
  let writing: WritingDocumentView
  let file: EditorFileView?
  let size: PaneLayoutSize
  let font: PaneLayoutFontMetric
  let accessibilityName: String
  let onEditingActivity: () -> Void
  let onSelectionActivity: (Bool) -> Void
  let onScrollActivity: () -> Void

  @State private var destinationRequest: PhoneWritingDestinationRequest?
  @State private var destination = ""

  private var plan: WritingLayoutPlan? {
    try? WritingLayoutPolicy(
      metrics: PhoneWritingMetrics.bodyWritingMetrics,
      floors: PhoneWritingMetrics.bodyFloors, gaps: PhoneWritingMetrics.sizing.gaps
    ).layout(
      WritingLayoutRequest(
        kind: .text,
        viewport: size,
        font: font, fullWidth: writing.document.fullWidth,
        reviewVisible: file?.review != nil))
  }

  var body: some View {
    if let plan, case .text(let textPlan) = plan.surface {
      ZStack(alignment: .topLeading) {
        PhonePalette.face
        if let review = plan.reviewRect {
          reviewView.phoneWritingFrame(review)
        }
        if let file, file.viewport != nil {
          PhoneWritingTextEditor(
            client: client, view: file, document: writing.document,
            accessibilityName: accessibilityName,
            onEditingActivity: onEditingActivity,
            onSelectionActivity: onSelectionActivity,
            onScrollActivity: onScrollActivity,
            onFormat: { kind, range in apply(kind, range: range) },
            onDestination: { kind, range in
              destination = ""
              destinationRequest = PhoneWritingDestinationRequest(kind: kind, range: range)
            }
          )
          .phoneWritingFrame(textPlan.columnRect)
          .clipShape(RoundedRectangle(cornerRadius: 10))
          .accessibilityIdentifier("phone-writing-text-\(writing.document.file.paneId)")
        } else {
          ProgressView().phoneWritingFrame(textPlan.columnRect)
        }
      }
      .frame(width: size.width, height: size.height, alignment: .topLeading)
      .sheet(item: $destinationRequest) { request in
        let sheetKind: PhoneWritingSheetKind = request.kind == .image ? .image : .link
        NavigationStack {
          Form {
            TextField(
              "", text: $destination,
              prompt: Text(request.kind == .image ? "Image path" : "Link destination")
                .foregroundStyle(PhonePalette.secondary)
            )
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .phoneWritingSheetRow()
          }
          .phoneWritingSheetScroll()
          .phoneWritingSheetNavigation(request.kind == .image ? "Insert image" : "Add link")
          .toolbar {
            ToolbarItem(placement: .cancellationAction) {
              Button("Cancel") { destinationRequest = nil }
            }
            ToolbarItem(placement: .confirmationAction) {
              Button("Apply") {
                apply(request.kind, range: request.range, value: destination)
                destinationRequest = nil
              }
              .disabled(destination.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
          }
        }
        .phoneWritingSheet(sheetKind)
      }
    }
  }

  private var reviewView: some View {
    Label("Agent changes ready to review", systemImage: "person.crop.circle.badge.checkmark")
      .font(.caption.bold())
      .foregroundStyle(PhonePalette.secondary)
      .padding(.horizontal, 10)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 10))
  }

  private func apply(
    _ kind: WritingFormatKind, range: SoyehtProtocol.TextRange, value: String? = nil
  ) {
    let fileId = writing.document.file.fileId
    Task { @MainActor in
      do {
        _ = try await client.formatWritingDocument(
          fileId: fileId, range: range, kind: kind, value: value)
      } catch {
        client.problem = "Could not format the selected text."
      }
    }
  }

}

private struct PhoneWritingDestinationRequest: Identifiable {
  let id = UUID()
  let kind: WritingFormatKind
  let range: SoyehtProtocol.TextRange
}

@MainActor
private struct PhoneWritingTextEditor: UIViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  let accessibilityName: String
  let onEditingActivity: () -> Void
  let onSelectionActivity: (Bool) -> Void
  let onScrollActivity: () -> Void
  let onFormat: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void
  let onDestination: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }

  func makeUIView(context: Context) -> UITextView {
    context.coordinator.makeTextView()
  }

  func updateUIView(_ textView: UITextView, context: Context) {
    context.coordinator.parent = self
    context.coordinator.update(textView)
  }

  @MainActor final class Coordinator: NSObject, UITextViewDelegate {
    var parent: PhoneWritingTextEditor
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedIdentity: String?
    private var renderedFont: WritingFont?
    private var renderedTheme: TerminalColorTheme?

    init(_ parent: PhoneWritingTextEditor) {
      self.parent = parent
    }

    func makeTextView() -> UITextView {
      let textView = UITextView()
      textView.delegate = self
      textView.isEditable = true
      textView.isSelectable = true
      textView.isScrollEnabled = true
      textView.alwaysBounceVertical = true
      textView.backgroundColor = UIColor(PhonePalette.face)
      textView.textContainerInset = .zero
      textView.textContainer.lineFragmentPadding = 0
      textView.keyboardDismissMode = .interactive
      textView.autocorrectionType = .yes
      textView.spellCheckingType = .yes
      textView.indicatorStyle = .default
      return textView
    }

    func update(_ textView: UITextView) {
      guard let viewport = parent.view.viewport else { return }
      firstLine = viewport.fromLine
      let identity = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      let theme = SoyehtThemeStore.shared.activeTheme
      let needsContent = identity != renderedIdentity
      let needsStyle = renderedFont != parent.document.font || renderedTheme != theme
      guard needsContent || needsStyle else { return }
      if needsContent { source = EditorEditing.viewportText(viewport.lines) }
      let selection = textView.selectedRange
      textView.attributedText = NSAttributedString(string: source, attributes: attributes())
      textView.selectedRange = validSelection(selection)
      textView.typingAttributes = attributes()
      textView.accessibilityLabel = "\(parent.accessibilityName), text document"
      renderedIdentity = identity
      renderedFont = parent.document.font
      renderedTheme = theme
    }

    func textView(
      _ textView: UITextView, shouldChangeTextIn range: NSRange,
      replacementText text: String
    ) -> Bool {
      let replacement = EditorLocalReplacement(range: range, text: text)
      let cursor = NSRange(location: range.location + text.utf16.count, length: 0)
      guard
        let plan = EditorEditing.replacementPlan(
          text: source, viewportStartLine: firstLine,
          replacements: [replacement], selections: [cursor])
      else { return false }
      source = EditorEditing.applying(plan.replacements, to: source)
      parent.client.enqueueEditorEdits(fileId: parent.view.file.fileId, edits: plan.edits)
      parent.onEditingActivity()
      return true
    }

    func textViewDidChangeSelection(_ textView: UITextView) {
      parent.onSelectionActivity(textView.selectedRange.length > 0)
    }

    func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
      parent.onScrollActivity()
    }

    func textView(
      _ textView: UITextView, editMenuForTextIn range: NSRange,
      suggestedActions: [UIMenuElement]
    ) -> UIMenu? {
      guard range.length > 0, let wireRange = wireRange(range) else {
        return UIMenu(children: suggestedActions)
      }
      let actions = PhoneWritingMetrics.chromePolicy.plan(
        kind: .text, hasSelection: true
      ).selectionActions
      let format = UIMenu(
        title: "Format", options: .displayInline,
        children: actions.compactMap { menuElement(for: $0, range: wireRange) })
      return UIMenu(children: suggestedActions + [format])
    }

    private func menuElement(
      for chromeAction: WritingChromeAction, range: SoyehtProtocol.TextRange
    ) -> UIMenuElement? {
      switch chromeAction {
      case .bold:
        action("Bold", image: "bold", kind: .bold, range: range)
      case .italic:
        action("Italic", image: "italic", kind: .italic, range: range)
      case .heading:
        UIMenu(
          title: "Heading", image: UIImage(systemName: "textformat.size"),
          children: [
            action("Heading 1", kind: .heading1, range: range),
            action("Heading 2", kind: .heading2, range: range),
            action("Heading 3", kind: .heading3, range: range),
          ])
      case .list:
        UIMenu(
          title: "List", image: UIImage(systemName: "list.bullet"),
          children: [
            action("Bulleted list", kind: .bullet_list, range: range),
            action("Numbered list", kind: .numbered_list, range: range),
          ])
      case .quote:
        action("Quote", image: "text.quote", kind: .quote, range: range)
      case .link:
        destinationAction("Link", image: "link", kind: .link, range: range)
      case .image:
        destinationAction("Image", image: "photo", kind: .image, range: range)
      case .font, .useFullWidth, .useReadingWidth, .focusMode, .ideaBoard, .scenes,
        .present, .splitColumns, .splitRows, .closePane:
        nil
      }
    }

    private func action(
      _ title: String, image: String? = nil, kind: WritingFormatKind,
      range: SoyehtProtocol.TextRange
    ) -> UIAction {
      UIAction(title: title, image: image.flatMap(UIImage.init(systemName:))) { [weak self] _ in
        guard let self else { return }
        Task { @MainActor in self.parent.onFormat(kind, range) }
      }
    }

    private func destinationAction(
      _ title: String, image: String, kind: WritingFormatKind,
      range: SoyehtProtocol.TextRange
    ) -> UIAction {
      UIAction(title: title, image: UIImage(systemName: image)) { [weak self] _ in
        guard let self else { return }
        Task { @MainActor in self.parent.onDestination(kind, range) }
      }
    }

    private func wireRange(_ range: NSRange) -> SoyehtProtocol.TextRange? {
      guard
        let start = EditorEditing.textPosition(
          text: source, viewportStartLine: firstLine, utf16Offset: range.location),
        let end = EditorEditing.textPosition(
          text: source, viewportStartLine: firstLine, utf16Offset: NSMaxRange(range))
      else { return nil }
      return SoyehtProtocol.TextRange(start: start, end: end)
    }

    private func attributes() -> [NSAttributedString.Key: Any] {
      let paragraph = NSMutableParagraphStyle()
      paragraph.minimumLineHeight = PaneLayoutContentMetrics.textLineHeight
      paragraph.maximumLineHeight = PaneLayoutContentMetrics.textLineHeight
      return [
        .font: PhoneWritingMetrics.uiFont(for: parent.document.font),
        .foregroundColor: UIColor(PhonePalette.ink),
        .paragraphStyle: paragraph,
      ]
    }

    private func validSelection(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else {
        return NSRange(location: 0, length: 0)
      }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }
  }
}

@MainActor
private struct PhoneWritingScenes: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let file: EditorFileView
  let structure: WritingStructureResult
  let accessibilityName: String

  @Environment(\.dismiss) private var dismiss
  private var sheetPlan: PhoneWritingSheetPlan {
    PhoneWritingMetrics.sheetPolicy.plan(for: .scenes)
  }

  var body: some View {
    NavigationStack {
      List(structure.scenes, id: \.sceneId) { scene in
        Button {
          open(scene)
        } label: {
          HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text("\(scene.ordinal + 1)")
              .font(.subheadline.monospacedDigit().bold())
              .foregroundStyle(PhonePalette.muted)
            Text(WritingDisplayText.sceneTitle(scene.heading))
              .lineLimit(2)
              .foregroundStyle(PhonePalette.ink)
          }
          .frame(minHeight: sheetPlan.minimumActionTarget)
        }
        .accessibilityLabel(
          "Scene \(scene.ordinal + 1), "
            + WritingDisplayText.sceneTitle(scene.heading)
        )
        .phoneWritingSheetRow()
      }
      .phoneWritingSheetScroll()
      .phoneWritingSheetNavigation("Scenes")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button {
            dismiss()
          } label: {
            PhoneWritingActionLabel(
              title: "Done", minimumTarget: sheetPlan.minimumActionTarget)
          }
        }
      }
    }
    .phoneWritingSheet(.scenes)
    .accessibilityLabel("\(accessibilityName), scenes")
  }

  private func open(_ scene: ScreenplayScene) {
    Task { @MainActor in
      do {
        try await client.loadEditorViewport(
          fileId: writing.document.file.fileId, fromLine: scene.line)
        dismiss()
      } catch {
        client.problem = "Could not open that scene."
      }
    }
  }
}

@MainActor
private struct PhoneWritingIdeaBoard: View {
  @Bindable var client: EngineClient
  let writing: WritingDocumentView
  let structure: WritingStructureResult
  let accessibilityName: String

  @Environment(\.dismiss) private var dismiss
  private var sheetPlan: PhoneWritingSheetPlan {
    PhoneWritingMetrics.sheetPolicy.plan(for: .ideaBoard)
  }

  var body: some View {
    NavigationStack {
      Group {
        if structure.beats.isEmpty {
          VStack(spacing: 8) {
            Image(systemName: "rectangle.3.group")
              .font(.title)
              .foregroundStyle(PhonePalette.muted)
            Text("No ideas")
              .font(.headline)
              .foregroundStyle(PhonePalette.ink)
          }
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .background(PhonePalette.canvas)
        } else {
          List(structure.beats.sorted { $0.order < $1.order }, id: \.beatId) { beat in
            VStack(alignment: .leading, spacing: 5) {
              Text(beat.title.isEmpty ? "Untitled idea" : beat.title)
                .font(.headline)
                .foregroundStyle(PhonePalette.ink)
              if !beat.body.isEmpty {
                Text(beat.body)
                  .font(.subheadline)
                  .foregroundStyle(PhonePalette.secondary)
                  .lineLimit(3)
              }
              Text(String(describing: beat.category).capitalized)
                .font(.caption)
                .foregroundStyle(PhonePalette.muted)
            }
            .frame(minHeight: sheetPlan.minimumActionTarget, alignment: .leading)
            .phoneWritingSheetRow()
          }
          .phoneWritingSheetScroll()
        }
      }
      .background(PhonePalette.canvas)
      .phoneWritingSheetNavigation("Idea board")
      .toolbar {
        ToolbarItem(placement: .confirmationAction) {
          Button {
            dismiss()
          } label: {
            PhoneWritingActionLabel(
              title: "Done", minimumTarget: sheetPlan.minimumActionTarget)
          }
        }
      }
    }
    .phoneWritingSheet(.ideaBoard)
    .accessibilityLabel("\(accessibilityName), idea board")
  }
}

extension View {
  func phoneWritingFrame(_ rect: PaneLayoutRect) -> some View {
    frame(width: rect.size.width, height: rect.size.height, alignment: .topLeading)
      .position(
        x: rect.origin.x + rect.size.width / 2,
        y: rect.origin.y + rect.size.height / 2)
  }

  fileprivate func phoneWritingSheet(_ kind: PhoneWritingSheetKind) -> some View {
    modifier(
      PhoneWritingSheetSurface(plan: PhoneWritingMetrics.sheetPolicy.plan(for: kind)))
  }

  fileprivate func phoneWritingSheetScroll() -> some View {
    scrollContentBackground(.hidden)
      .background(PhonePalette.canvas)
  }

  fileprivate func phoneWritingSheetRow() -> some View {
    foregroundStyle(PhonePalette.ink)
      .listRowBackground(PhonePalette.face)
  }

  fileprivate func phoneWritingSheetNavigation(_ title: String) -> some View {
    navigationTitle(title)
      .navigationBarTitleDisplayMode(.inline)
      .toolbar {
        ToolbarItem(placement: .principal) {
          Text(title)
            .font(.headline)
            .foregroundStyle(PhonePalette.ink)
        }
      }
  }
}

private struct PhoneWritingSheetSurface: ViewModifier {
  let plan: PhoneWritingSheetPlan

  func body(content: Content) -> some View {
    content
      .foregroundStyle(PhonePalette.ink)
      .tint(PhonePalette.accent)
      .background(PhonePalette.canvas.ignoresSafeArea())
      .toolbarBackground(PhonePalette.face, for: .navigationBar)
      .toolbarBackground(.visible, for: .navigationBar)
      .presentationDetents(Set(plan.detents.map(\.presentationDetent)))
  }
}

extension PhoneWritingSheetDetent {
  fileprivate var presentationDetent: PresentationDetent {
    switch self {
    case .medium: .medium
    case .large: .large
    }
  }
}
