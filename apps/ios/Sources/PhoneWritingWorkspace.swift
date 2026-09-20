import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

@MainActor
struct PhoneWritingWorkspace: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let initialPaneId: String

  @State private var localState: PhoneWritingLocalState?
  @State private var showingNewText = false
  @State private var newTextTitle = ""
  @State private var creatingText = false
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
                  pane: pane(withId: leaf.identity.paneId), floors: PhoneWritingMetrics.floors,
                  onAction: { action in apply(action, to: plan) }
                )
                .phoneWritingFrame(leaf.rowRect)
              }
            }
          }
          .scrollBounceBehavior(.basedOnSize)
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
        viewport: PaneLayoutSize(width: viewport.width, height: viewport.height)))
  }

  private func layoutTree(livePanes: [Pane]) -> PaneLayoutTree? {
    if let reconciliation = try? client.paneLayouts.reconciledPresentation(
      for: workspaceId, livePanes: livePanes, sizing: PhoneWritingMetrics.sizing)
    {
      return reconciliation.state.tree
    }
    guard let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId }) else {
      return nil
    }
    var state = client.paneLayoutState(for: workspace)
    _ = try? state.reconcile(livePanes: livePanes, sizing: PhoneWritingMetrics.sizing)
    return state.tree
  }

  private func apply(_ action: PhoneWritingStateAction, to plan: PhoneWritingLayoutPlan) {
    let updated = PhoneWritingMetrics.policy.applying(action, to: plan)
    localState = updated
    persist(updated)
  }

  private func reconcile(_ state: PhoneWritingLocalState) {
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

  private func createText() {
    let title = newTextTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !title.isEmpty, !creatingText else { return }
    creatingText = true
    Task { @MainActor in
      defer { creatingText = false }
      do {
        let result = try await client.createWritingDocument(
          workspaceId: workspaceId, section: .texts, title: title)
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
    floors: floors, gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))
  static let policy = PhoneWritingLayoutPolicy(floors: floors)

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
  let floors: [PaneLayoutKind: PaneLayoutSize]
  let onAction: (PhoneWritingStateAction) -> Void

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
      Button {
        toggle()
      } label: {
        HStack(spacing: 10) {
          Image(systemName: icon)
            .foregroundStyle(PhonePalette.accent)
            .frame(width: 24)
          Text(pane?.name ?? leaf.name)
            .font(.system(.headline, design: .rounded))
            .lineLimit(1)
            .frame(maxWidth: .infinity, alignment: .leading)
          if leaf.companionAgentPaneId != nil {
            Image(systemName: "link")
              .foregroundStyle(PhonePalette.secondary)
              .accessibilityLabel("Linked agent")
          }
          Image(systemName: leaf.expanded ? "chevron.up" : "chevron.down")
            .foregroundStyle(PhonePalette.secondary)
        }
        .padding(.horizontal, 14)
        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44)
        .contentShape(Rectangle())
      }
      .buttonStyle(.plain)
      .accessibilityLabel(accessibilityLabel)
      .accessibilityValue(leaf.expanded ? "Expanded" : "Collapsed")
      .accessibilityHint(leaf.expanded ? "Collapses this document" : "Opens this document")
      .accessibilityIdentifier("phone-writing-header-\(leaf.identity.paneId)")

      if leaf.companionAgentPaneId != nil {
        HStack(spacing: 0) {
          tabButton(.document, title: "Document", systemImage: icon)
          tabButton(.agent, title: "Agent", systemImage: "bubble.left.and.bubble.right")
        }
        .frame(height: 44)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Linked document and agent tabs")
      }
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
          floors: floors, accessibilityName: leaf.name)
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
            floors: floors, accessibilityName: leaf.name)
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
        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44)
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
  let floors: [PaneLayoutKind: PaneLayoutSize]
  let accessibilityName: String

  private var plan: WritingLayoutPlan? {
    try? WritingLayoutPolicy(
      floors: floors, gaps: PaneLayoutGaps(horizontal: 18, vertical: 18)
    ).layout(
      WritingLayoutRequest(
        kind: .text,
        viewport: PaneLayoutSize(
          width: size.width,
          height: size.height + WritingLayoutMetrics.standard.headerHeight),
        font: font, fullWidth: writing.document.fullWidth,
        reviewVisible: file?.review != nil))
  }

  var body: some View {
    if let plan, case .text(let textPlan) = plan.surface {
      ZStack(alignment: .topLeading) {
        PhonePalette.canvas
        if let toolbar = translated(plan.toolbarRect) {
          toolbarView.phoneWritingFrame(toolbar)
        }
        if let review = translated(plan.reviewRect) {
          reviewView.phoneWritingFrame(review)
        }
        if let file, file.viewport != nil {
          PhoneWritingTextEditor(
            client: client, view: file, document: writing.document,
            accessibilityName: accessibilityName
          )
          .phoneWritingFrame(translated(textPlan.columnRect))
          .clipShape(RoundedRectangle(cornerRadius: 10))
          .accessibilityIdentifier("phone-writing-text-\(writing.document.file.paneId)")
        } else {
          ProgressView().phoneWritingFrame(translated(textPlan.columnRect))
        }
      }
      .frame(width: size.width, height: size.height, alignment: .topLeading)
    }
  }

  private var toolbarView: some View {
    HStack(spacing: 8) {
      Menu {
        fontButton("Newsreader", .reading_serif)
        fontButton("Classic serif", .classic_serif)
        fontButton("Sans", .sans)
      } label: {
        Label(fontLabel, systemImage: "textformat")
          .lineLimit(1)
      }
      Spacer(minLength: 4)
      if file?.conflict != nil {
        Image(systemName: "exclamationmark.triangle.fill")
          .foregroundStyle(PhonePalette.warning)
          .accessibilityLabel("File conflict")
      }
      Button {
        setFullWidth(!writing.document.fullWidth)
      } label: {
        Image(
          systemName: writing.document.fullWidth
            ? "arrow.down.right.and.arrow.up.left"
            : "arrow.up.left.and.arrow.down.right"
        )
        .frame(width: 32, height: 32)
      }
      .accessibilityLabel(
        writing.document.fullWidth ? "Use reading width" : "Use full width"
      )
      .accessibilityIdentifier("phone-writing-full-width-\(writing.document.file.paneId)")
    }
    .font(.system(.subheadline, design: .rounded, weight: .semibold))
    .foregroundStyle(PhonePalette.secondary)
    .padding(.horizontal, 10)
    .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 10))
  }

  private var reviewView: some View {
    Label("Agent changes ready to review", systemImage: "person.crop.circle.badge.checkmark")
      .font(.caption.bold())
      .foregroundStyle(PhonePalette.secondary)
      .padding(.horizontal, 10)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      .background(PhonePalette.face, in: RoundedRectangle(cornerRadius: 10))
  }

  private var fontLabel: String {
    switch writing.document.font {
    case .reading_serif: "Newsreader"
    case .classic_serif: "Classic serif"
    case .sans: "Sans"
    }
  }

  private func fontButton(_ title: String, _ font: WritingFont) -> some View {
    Button {
      setFont(font)
    } label: {
      if writing.document.font == font {
        Label(title, systemImage: "checkmark")
      } else {
        Text(title)
      }
    }
  }

  private func setFont(_ font: WritingFont) {
    let fileId = writing.document.file.fileId
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, font: font) } catch {
        client.problem = "Could not change the document font."
      }
    }
  }

  private func setFullWidth(_ enabled: Bool) {
    let fileId = writing.document.file.fileId
    Task { @MainActor in
      do { _ = try await client.setWritingPreferences(fileId: fileId, fullWidth: enabled) } catch {
        client.problem = "Could not change the document width."
      }
    }
  }

  private func translated(_ rect: PaneLayoutRect?) -> PaneLayoutRect? {
    rect.map(translated)
  }

  private func translated(_ rect: PaneLayoutRect) -> PaneLayoutRect {
    PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: rect.origin.x,
        y: rect.origin.y - WritingLayoutMetrics.standard.headerHeight),
      size: rect.size)
  }
}

@MainActor
private struct PhoneWritingTextEditor: UIViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let document: WritingDocument
  let accessibilityName: String

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
      return true
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

extension View {
  func phoneWritingFrame(_ rect: PaneLayoutRect) -> some View {
    frame(width: rect.size.width, height: rect.size.height, alignment: .topLeading)
      .position(
        x: rect.origin.x + rect.size.width / 2,
        y: rect.origin.y + rect.size.height / 2)
  }
}
