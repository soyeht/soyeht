import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingPaneHeaderAction {
  let title: String
  let systemImage: String
  let help: String
  let accessibilityIdentifier: String
  var disabled = false
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
  var companionLinked = false
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
      if companionLinked {
        Image(systemName: "link")
          .help("Linked writing companion")
          .accessibilityLabel("Linked writing companion")
          .accessibilityIdentifier("writing-companion-link-\(paneId)")
      }
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
        .disabled(auxiliaryAction.disabled)
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

/// The accepted P5/P2 writing header. Screenplay and presentation adopt this in E2/E3.
struct WritingChromeHeader<Trailing: View>: View {
  let paneId: String
  let title: String
  let metadata: String?
  let companionLinked: Bool
  let opacity: Double
  let headerDrag: PaneHeaderDragConfiguration?
  let onHover: (Bool) -> Void
  @ViewBuilder let trailing: () -> Trailing

  var body: some View {
    HStack(spacing: WritingChromeMetrics.standard.headerActionGap) {
      HStack(spacing: MacTheme.Space.x6) {
        Text(title)
          .font(
            MacTheme.chrome(
              WritingChromeMetrics.standard.headerFontPointSize, weight: .semibold)
          )
          .foregroundStyle(MacTheme.secondary)
          .lineLimit(1)
          .help(title)
          .accessibilityLabel(title)
        if companionLinked {
          Image(systemName: "link")
            .font(.system(size: WritingChromeMetrics.standard.headerMenuIconSize))
            .foregroundStyle(MacTheme.secondary)
            .help("Linked writing companion")
            .accessibilityLabel("Linked writing companion")
            .accessibilityIdentifier("writing-companion-link-\(paneId)")
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      if let metadata {
        Text(metadata)
          .font(MacTheme.chrome(WritingChromeMetrics.standard.headerFontPointSize))
          .foregroundStyle(MacTheme.muted)
          .lineLimit(1)
      }
      trailing()
    }
    .padding(.horizontal, WritingChromeMetrics.standard.headerHorizontalInset)
    .frame(height: WritingChromeMetrics.standard.headerHeight)
    .background(MacTheme.face)
    .opacity(opacity)
    .onHover(perform: onHover)
    .paneHeaderDrag(headerDrag)
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("writing-header-\(paneId)")
  }
}

struct WritingTextPane: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var selection: SoyehtProtocol.TextRange?
  @State private var selectionGeometry: PaneLayoutRect?
  @State private var activeLine: UInt32 = 0
  @State private var destinationKind: WritingFormatKind?
  @State private var destination = ""
  @State private var showingOptions = false
  @State private var showingCloseConfirmation = false
  @State private var chromeActivity = WritingChromeActivityState()

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
            .forceEnclosingOverlayScrollers()
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDisabled(plan.overflowAxes.isEmpty)
        .frame(width: proxy.size.width, height: proxy.size.height)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
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
    .onChange(of: hasSelection) { _, value in
      updateChrome(.selectionChanged(value))
      if !value { selectionGeometry = nil }
    }
    .onChange(of: showingOptions) { _, value in updateChrome(.menuChanged(value)) }
    .onChange(of: destinationKind != nil) { _, value in
      updateChrome(.popoverChanged(value))
    }
    .alert("Close \(documentFileName)?", isPresented: $showingCloseConfirmation) {
      Button("Cancel", role: .cancel) {}
      Button("Close", role: .destructive) { client.closePane(pane.paneId) }
    } message: {
      Text("Unsaved changes will be lost.")
    }
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
      MacTheme.face
        .writingFrame(plan.contentRect)
      writingHeader(document: document)
        .writingFrame(plan.headerRect)
      if let reviewRect = plan.reviewRect, let file {
        WritingTextReviewStrip(
          client: client, view: file, selectedPosition: selection?.start
        )
        .writingFrame(reviewRect)
      }
      if let file, file.viewport != nil,
        case .text(let textPlan) = plan.surface
      {
        WritingTextViewport(
          client: client, view: file, document: document,
          selection: $selection, selectionGeometry: $selectionGeometry,
          activeLine: $activeLine, onEditingActivity: { updateChrome(.edited) },
          onFormatShortcut: { kind, range in apply(kind, range: range) }
        )
        .writingFrame(textPlan.columnRect)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
        .accessibilityIdentifier("writing-text-column-\(pane.paneId)")
      } else if case .text(let textPlan) = plan.surface {
        ProgressView()
          .writingFrame(textPlan.columnRect)
      }
      if let selectionGeometry,
        let toolbarRect = selectionToolbarRect(selectionGeometry, plan: plan)
      {
        selectionToolbar
          .writingFrame(toolbarRect)
          .zIndex(2)
      }
    }
    .frame(
      width: plan.contentSize.width, height: plan.contentSize.height,
      alignment: .topLeading
    )
    .onContinuousHover { phase in
      if case .active = phase { updateChrome(.pointerMoved) }
    }
  }

  private func writingHeader(document: WritingDocument?) -> some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: documentFileName,
      metadata: wordCountLabel,
      companionLinked: companionLinked, opacity: chromeOpacity,
      headerDrag: headerDrag,
      onHover: { updateChrome(.headerHoverChanged($0)) },
      trailing: {
        Button {
          showingOptions.toggle()
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
        .help("Writing options")
        .accessibilityLabel("Writing options menu")
        .accessibilityIdentifier("writing-options-\(pane.paneId)")
        .popover(isPresented: $showingOptions, arrowEdge: .bottom) {
          if let document { writingOptions(document) }
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

  private func writingOptions(_ document: WritingDocument) -> some View {
    let groups = MacPaneLayoutMetrics.chromePolicy.plan(
      kind: .text, fullWidth: document.fullWidth
    ).menuGroups
    return VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
        if index > 0 { Divider().padding(.vertical, MacTheme.Space.x3) }
        ForEach(group, id: \.rawValue) { action in
          writingOption(action, document: document)
        }
      }
    }
    .padding(MacTheme.Space.x8)
    .frame(width: PaneLayoutContentMetrics.writingOptionsMenuWidth)
    .background(MacTheme.face)
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Writing options menu")
  }

  @ViewBuilder private func writingOption(
    _ action: WritingChromeAction, document: WritingDocument
  ) -> some View {
    switch action {
    case .font:
      Menu {
        fontButton(
          WritingDisplayText.fontName(.reading_serif), .reading_serif, document: document)
        fontButton(
          WritingDisplayText.fontName(.classic_serif), .classic_serif, document: document)
        fontButton(WritingDisplayText.fontName(.sans), .sans, document: document)
      } label: {
        optionLabel(WritingDisplayText.fontName(document.font), icon: "textformat")
      }
      .menuStyle(.borderlessButton)
      .focusable(false)
      .disabled(!client.connected)
      .accessibilityLabel("Font: \(WritingDisplayText.fontName(document.font))")
      .accessibilityIdentifier("writing-options-font-\(pane.paneId)")
    case .useFullWidth, .useReadingWidth:
      let fullWidth = action == .useFullWidth
      optionButton(
        fullWidth ? "Use full width" : "Use reading width",
        icon: fullWidth
          ? "arrow.up.left.and.arrow.down.right" : "arrow.down.right.and.arrow.up.left",
        disabled: !client.connected,
        identifier: "writing-options-width-\(pane.paneId)"
      ) {
        showingOptions = false
        setFullWidth(fullWidth)
      }
    case .focusMode:
      optionButton(
        document.focusMode ? "Turn off Focus mode" : "Focus mode", icon: "scope",
        active: document.focusMode, disabled: !client.connected,
        identifier: "writing-options-focus-\(pane.paneId)"
      ) {
        showingOptions = false
        setFocus(!document.focusMode)
      }
    case .splitColumns:
      optionButton(
        "Split into columns", icon: "rectangle.split.2x1",
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "split-pane-columns-\(pane.paneId)"
      ) {
        showingOptions = false
        onSplit(pane.paneId, .columns)
      }
    case .splitRows:
      optionButton(
        "Split into rows", icon: "rectangle.split.1x2",
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "split-pane-rows-\(pane.paneId)"
      ) {
        showingOptions = false
        onSplit(pane.paneId, .rows)
      }
    case .closePane:
      optionButton(
        "Close pane", icon: "xmark", role: .destructive,
        disabled: !client.connected || layoutActionsDisabled,
        identifier: "close-pane-\(pane.paneId)"
      ) {
        showingOptions = false
        requestClose()
      }
    case .ideaBoard, .scenes, .present, .bold, .italic, .heading, .list, .quote, .link,
      .image:
      EmptyView()
    }
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
      .disabled(disabled)
      .accessibilityIdentifier(identifier)
  }

  private func fontButton(
    _ title: String, _ font: WritingFont, document: WritingDocument
  ) -> some View {
    Button {
      showingOptions = false
      setFont(font)
    } label: {
      if document.font == font { Label(title, systemImage: "checkmark") } else { Text(title) }
    }
  }

  private var selectionToolbar: some View {
    HStack(spacing: WritingChromeMetrics.standard.toolbarGap) {
      selectionFormatButton("bold", "Bold", .bold, identifier: "bold")
        .keyboardShortcut("b", modifiers: .command)
      selectionFormatButton("italic", "Italic", .italic, identifier: "italic")
        .keyboardShortcut("i", modifiers: .command)
      Menu {
        Button("Heading 1") { apply(.heading1) }
        Button("Heading 2") { apply(.heading2) }
        Button("Heading 3") { apply(.heading3) }
      } label: {
        selectionToolbarIcon("textformat.size", help: "Heading")
      }
      .menuStyle(.borderlessButton)
      .menuIndicator(.hidden)
      .frame(
        width: WritingChromeMetrics.standard.toolbarButtonWidth,
        height: WritingChromeMetrics.standard.toolbarSize.height
      )
      .help("Heading")
      .accessibilityLabel("Heading")
      .accessibilityIdentifier("writing-selection-heading-\(pane.paneId)")
      Menu {
        Button("Bulleted list") { apply(.bullet_list) }
        Button("Numbered list") { apply(.numbered_list) }
      } label: {
        selectionToolbarIcon("list.bullet", help: "List")
      }
      .menuStyle(.borderlessButton)
      .menuIndicator(.hidden)
      .frame(
        width: WritingChromeMetrics.standard.toolbarButtonWidth,
        height: WritingChromeMetrics.standard.toolbarSize.height
      )
      .help("List")
      .accessibilityLabel("List")
      .accessibilityIdentifier("writing-selection-list-\(pane.paneId)")
      selectionFormatButton("text.quote", "Quote", .quote, identifier: "quote")
      selectionDestinationButton("link", "Link", .link, identifier: "link")
        .keyboardShortcut("k", modifiers: .command)
      selectionDestinationButton("photo", "Image", .image, identifier: "image")
    }
    .padding(.horizontal, WritingChromeMetrics.standard.toolbarHorizontalInset)
    .frame(
      width: WritingChromeMetrics.standard.toolbarSize.width,
      height: WritingChromeMetrics.standard.toolbarSize.height
    )
    .background(MacTheme.face, in: Capsule())
    .shadow(
      color: MacTheme.shadowDark.opacity(0.40),
      radius: WritingChromeMetrics.standard.toolbarShadowBlur / 2,
      x: 0, y: WritingChromeMetrics.standard.toolbarShadowOffset
    )
    .popover(
      isPresented: Binding(
        get: { destinationKind != nil },
        set: { if !$0 { destinationKind = nil } }),
      arrowEdge: .bottom
    ) {
      destinationPopover
    }
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Formatting selection")
    .accessibilityIdentifier("writing-selection-toolbar-\(pane.paneId)")
    .disabled(!client.connected)
  }

  private func selectionFormatButton(
    _ icon: String, _ help: String, _ kind: WritingFormatKind, identifier: String
  ) -> some View {
    Button {
      apply(kind)
    } label: {
      selectionToolbarIcon(icon, help: help)
    }
    .buttonStyle(.plain)
    .help(help)
    .accessibilityLabel(help)
    .accessibilityIdentifier("writing-selection-\(identifier)-\(pane.paneId)")
  }

  private func selectionDestinationButton(
    _ icon: String, _ help: String, _ kind: WritingFormatKind, identifier: String
  ) -> some View {
    Button {
      guard hasSelection else { return }
      destination = ""
      destinationKind = kind
    } label: {
      selectionToolbarIcon(icon, help: help)
    }
    .buttonStyle(.plain)
    .help(help)
    .accessibilityLabel(help)
    .accessibilityIdentifier("writing-selection-\(identifier)-\(pane.paneId)")
  }

  private func selectionToolbarIcon(_ icon: String, help: String) -> some View {
    Image(systemName: icon)
      .font(
        .system(
          size: WritingChromeMetrics.standard.toolbarIconSize,
          weight: .semibold)
      )
      .foregroundStyle(MacTheme.ink)
      .frame(
        width: WritingChromeMetrics.standard.toolbarButtonWidth,
        height: WritingChromeMetrics.standard.toolbarSize.height
      )
      .contentShape(Rectangle())
      .help(help)
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
    .padding(MacTheme.Space.x16).frame(width: 320).background(MacTheme.face)
  }

  private var wordCountLabel: String? {
    guard let count = writing?.structure?.wordCount else { return nil }
    return WritingDisplayText.count(Int(count), kind: .word)
  }

  private var documentFileName: String {
    guard let path = writing?.document.file.path else { return pane.name }
    return URL(fileURLWithPath: path).lastPathComponent
  }

  private var hasSelection: Bool {
    guard let selection else { return false }
    return selection.start.line != selection.end.line
      || selection.start.column != selection.end.column
  }

  private var chromeOpacity: Double {
    chromeActivity.visibility == .dimmed
      ? WritingChromeMetrics.standard.dimmedOpacity : 1
  }

  private func selectionToolbarRect(
    _ localSelection: PaneLayoutRect, plan: WritingLayoutPlan
  ) -> PaneLayoutRect? {
    guard hasSelection, case .text(let textPlan) = plan.surface else { return nil }
    let selectionInSurface = PaneLayoutRect(
      origin: PaneLayoutPoint(
        x: textPlan.columnRect.origin.x + localSelection.origin.x,
        y: textPlan.columnRect.origin.y + localSelection.origin.y),
      size: localSelection.size)
    return MacPaneLayoutMetrics.chromePolicy.selectionToolbarRect(
      selection: selectionInSurface, within: plan.contentRect)
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
    switch MacPaneLayoutMetrics.chromePolicy.closeDisposition(
      fileName: documentFileName, dirty: file?.file.dirty == true)
    {
    case .closeImmediately:
      client.closePane(pane.paneId)
    case .confirm:
      showingCloseConfirmation = true
    }
  }

  private func apply(
    _ kind: WritingFormatKind, value: String? = nil,
    range explicitRange: SoyehtProtocol.TextRange? = nil
  ) {
    guard let fileId = writing?.document.file.fileId,
      let range = explicitRange ?? selection,
      range.start.line != range.end.line || range.start.column != range.end.column
    else { return }
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
        client: client, view: view, selectedPosition: selectedPosition
      )
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
  @Binding var selectionGeometry: PaneLayoutRect?
  @Binding var activeLine: UInt32
  let onEditingActivity: () -> Void
  let onFormatShortcut: (WritingFormatKind, SoyehtProtocol.TextRange) -> Void

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
    var parent: WritingTextViewport
    private let textView = WritingImagePasteTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var renderedRevision: String?
    private var renderedFont: WritingFont?
    private var renderedTheme: TerminalColorTheme?
    private var rendering = false
    private weak var scrollView: NSScrollView?

    init(_ parent: WritingTextViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.autohidesScrollers = true
      scroll.scrollerStyle = .overlay
      scroll.borderType = .noBorder
      scroll.documentView = textView
      scroll.contentView.postsBoundsChangedNotifications = true
      NotificationCenter.default.addObserver(
        self, selector: #selector(viewportDidScroll(_:)),
        name: NSView.boundsDidChangeNotification, object: scroll.contentView)
      scrollView = scroll
      configureTextView()
      textView.writingFormatShortcutHandler = { [weak self] kind in
        self?.applyFormatShortcut(kind)
      }
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
      publishSelectionGeometry(deferred: true)
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
      publishSelectionGeometry()
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
      parent.onEditingActivity()
      source = raw.replacingCharacters(in: affectedCharRange, with: replacement)
      let cursor = affectedCharRange.location + replacement.utf16.count
      let cursorPosition = position(cursor) ?? start
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId,
        edits: [
          TextEdit(range: SoyehtProtocol.TextRange(start: start, end: end), text: replacement)
        ])
      parent.activeLine = cursorPosition.line
      parent.selection = SoyehtProtocol.TextRange(start: cursorPosition, end: cursorPosition)
      parent.selectionGeometry = nil
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
      publishSelectionGeometry(deferred: true)
    }

    @objc private func viewportDidScroll(_ notification: Notification) {
      publishSelectionGeometry()
    }

    private func publishSelectionGeometry(deferred: Bool = false) {
      if deferred {
        Task { @MainActor [weak self] in self?.publishSelectionGeometry() }
        return
      }
      guard scrollView != nil else {
        parent.selectionGeometry = nil
        return
      }
      let selectedRange = textView.selectedRange()
      guard selectedRange.length > 0, let layoutManager = textView.layoutManager,
        let textContainer = textView.textContainer
      else {
        parent.selectionGeometry = nil
        return
      }
      let glyphRange = layoutManager.glyphRange(
        forCharacterRange: selectedRange, actualCharacterRange: nil)
      var rect = layoutManager.boundingRect(forGlyphRange: glyphRange, in: textContainer)
      rect.origin.x += textView.textContainerOrigin.x
      rect.origin.y += textView.textContainerOrigin.y
      let visible = textView.visibleRect
      let clipped = rect.intersection(visible)
      guard !clipped.isNull, clipped.width > 0, clipped.height > 0 else {
        parent.selectionGeometry = nil
        return
      }
      parent.selectionGeometry = PaneLayoutRect(
        origin: PaneLayoutPoint(
          x: clipped.minX - visible.minX,
          y: clipped.minY - visible.minY),
        size: PaneLayoutSize(width: clipped.width, height: clipped.height))
    }

    private func applyFormatShortcut(_ kind: WritingFormatKind) {
      let selectedRange = textView.selectedRange()
      guard selectedRange.length > 0,
        let start = position(selectedRange.location),
        let end = position(NSMaxRange(selectedRange))
      else { return }
      parent.onFormatShortcut(
        kind, SoyehtProtocol.TextRange(start: start, end: end))
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
