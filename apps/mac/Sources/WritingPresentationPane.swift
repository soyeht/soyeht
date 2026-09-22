import AppKit
import Observation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingSlideShowRequest: Sendable {
  let paneId: String
  let fileId: String
  let initialIndex: Int
  let slideCount: Int
  let viewportOffset: PaneLayoutPoint?
}

struct WritingSlideShowAction: Sendable {
  private let handler: (@MainActor @Sendable (WritingSlideShowRequest) -> Void)?

  init(_ handler: @escaping @MainActor @Sendable (WritingSlideShowRequest) -> Void) {
    self.handler = handler
  }

  fileprivate init() { handler = nil }

  @MainActor func callAsFunction(_ request: WritingSlideShowRequest) {
    handler?(request)
  }
}

private struct WritingSlideShowActionKey: EnvironmentKey {
  static let defaultValue = WritingSlideShowAction()
}

extension EnvironmentValues {
  var writingSlideShowAction: WritingSlideShowAction {
    get { self[WritingSlideShowActionKey.self] }
    set { self[WritingSlideShowActionKey.self] = newValue }
  }

  var writingSlideShowViewportRegistry: WritingPresentationViewportRegistry? {
    get { self[WritingSlideShowViewportRegistryKey.self] }
    set { self[WritingSlideShowViewportRegistryKey.self] = newValue }
  }
}

@MainActor final class WritingPresentationViewportRegistry: @unchecked Sendable {
  private final class WeakScrollView {
    weak var value: NSScrollView?

    init(_ value: NSScrollView) { self.value = value }
  }

  private var scrollViews: [String: WeakScrollView] = [:]
  private var pendingRestorations: [String: PaneLayoutPoint] = [:]
  private var restorationTasks: [String: Task<Void, Never>] = [:]

  func attach(_ scrollView: NSScrollView, paneId: String) {
    scrollViews[paneId] = WeakScrollView(scrollView)
    if applyPendingRestoration(for: paneId) {
      pendingRestorations[paneId] = nil
    }
  }

  func detach(_ scrollView: NSScrollView, paneId: String) {
    guard scrollViews[paneId]?.value === scrollView else { return }
    scrollViews[paneId] = nil
  }

  func capture(paneId: String) -> PaneLayoutPoint? {
    guard let origin = scrollViews[paneId]?.value?.contentView.bounds.origin else { return nil }
    return PaneLayoutPoint(x: origin.x, y: origin.y)
  }

  func restore(_ offset: PaneLayoutPoint, paneId: String) {
    pendingRestorations[paneId] = offset
    restorationTasks[paneId]?.cancel()
    restorationTasks[paneId] = Task { @MainActor [weak self] in
      await Task.yield()
      guard !Task.isCancelled, let self else { return }
      var restored = applyPendingRestoration(for: paneId)
      await Task.yield()
      guard !Task.isCancelled else { return }
      restored = applyPendingRestoration(for: paneId) || restored
      if restored { pendingRestorations[paneId] = nil }
      restorationTasks[paneId] = nil
    }
  }

  @discardableResult private func applyPendingRestoration(for paneId: String) -> Bool {
    guard let scrollView = scrollViews[paneId]?.value,
      let offset = pendingRestorations[paneId]
    else { return false }
    scrollView.layoutSubtreeIfNeeded()
    scrollView.contentView.scroll(to: NSPoint(x: offset.x, y: offset.y))
    scrollView.reflectScrolledClipView(scrollView.contentView)
    return true
  }
}

private struct WritingSlideShowViewportRegistryKey: EnvironmentKey {
  static let defaultValue: WritingPresentationViewportRegistry? = nil
}

struct WritingSlideShowSession: Equatable, Sendable {
  let paneId: String
  let fileId: String
}

@MainActor @Observable final class MacWritingSlideShowCoordinator {
  private final class WindowReference {
    weak var value: NSWindow?
  }

  private struct WindowSnapshot {
    let frame: NSRect
    let firstResponder: NSResponder?
    let acceptsMouseMovedEvents: Bool
  }

  private(set) var session: WritingSlideShowSession?
  private(set) var navigation = WritingSlideShowState(index: 0, slideCount: 0)
  @ObservationIgnored private let policy = WritingSlideShowPolicy.standard
  @ObservationIgnored private let windowReference = WindowReference()
  @ObservationIgnored private var windowState = WritingSlideShowWindowState()
  @ObservationIgnored private var cursorState = WritingSlideShowCursorState()
  @ObservationIgnored private var snapshot: WindowSnapshot?
  @ObservationIgnored private var inputMonitor: Any?
  @ObservationIgnored private var cursorTask: Task<Void, Never>?
  @ObservationIgnored private var fullScreenEntryTask: Task<Void, Never>?
  @ObservationIgnored private var viewportOffsetState = WritingSlideShowScrollOffsetState()
  @ObservationIgnored let viewportRegistry = WritingPresentationViewportRegistry()

  var isPresenting: Bool { session != nil }

  func attach(window: NSWindow?) {
    guard windowReference.value !== window else { return }
    if isPresenting { forceFinish() }
    windowReference.value = window
  }

  func present(_ request: WritingSlideShowRequest) {
    guard session == nil, request.slideCount > 0, let window = windowReference.value else {
      return
    }
    session = WritingSlideShowSession(paneId: request.paneId, fileId: request.fileId)
    navigation = WritingSlideShowState(
      index: request.initialIndex, slideCount: request.slideCount)
    if let viewportOffset = request.viewportOffset {
      viewportOffsetState.capture(viewportOffset)
    }
    snapshot = WindowSnapshot(
      frame: window.frame, firstResponder: window.firstResponder,
      acceptsMouseMovedEvents: window.acceptsMouseMovedEvents)
    window.makeFirstResponder(nil)
    window.acceptsMouseMovedEvents = true
    installInputMonitor()
    perform(
      windowState.begin(
        isFullScreen: window.styleMask.contains(.fullScreen),
        canEnterFullScreen: !window.collectionBehavior.contains(.fullScreenNone)))
  }

  func reconcile(paneExists: Bool, connected: Bool, slideCount: Int) {
    guard isPresenting else { return }
    guard paneExists, connected else {
      requestExit()
      return
    }
    if navigation.reconcile(slideCount: slideCount) { requestExit() }
  }

  func requestExit() {
    guard isPresenting else { return }
    perform(windowState.requestExit())
  }

  func windowDidEnterFullScreen(_ window: NSWindow?) {
    guard owns(window) else { return }
    cancelFullScreenEntryTimeout()
    perform(windowState.didEnterFullScreen())
  }

  func windowDidExitFullScreen(_ window: NSWindow?) {
    guard owns(window) else { return }
    perform(windowState.didExitFullScreen())
  }

  func windowDidBecomeKey(_ window: NSWindow?) {
    guard owns(window), isPresenting else { return }
    noteActivity()
  }

  func windowDidResignKey(_ window: NSWindow?) {
    guard owns(window), isPresenting else { return }
    cursorTask?.cancel()
    cursorTask = nil
    applyCursor(cursorState.reduce(.activity))
  }

  func windowWillClose(_ window: NSWindow?) {
    guard owns(window) else { return }
    forceFinish()
  }

  private func owns(_ window: NSWindow?) -> Bool {
    guard let window, let owned = windowReference.value else { return false }
    return window === owned
  }

  private func perform(_ effect: WritingSlideShowWindowEffect) {
    guard let window = windowReference.value else {
      forceFinish()
      return
    }
    switch effect {
    case .none:
      break
    case .enterFullScreen:
      scheduleFullScreenEntryTimeout()
      window.toggleFullScreen(nil)
    case .exitFullScreen:
      window.toggleFullScreen(nil)
    case .activate:
      noteActivity()
    case .finish:
      finish(window: window)
    }
  }

  private func scheduleFullScreenEntryTimeout() {
    cancelFullScreenEntryTimeout()
    let delay = policy.fullScreenEntryTimeout
    fullScreenEntryTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .seconds(delay))
      guard !Task.isCancelled, let self else { return }
      self.fullScreenEntryTask = nil
      self.perform(self.windowState.didFailToEnterFullScreen())
    }
  }

  private func cancelFullScreenEntryTimeout() {
    fullScreenEntryTask?.cancel()
    fullScreenEntryTask = nil
  }

  private func installInputMonitor() {
    guard inputMonitor == nil else { return }
    inputMonitor = NSEvent.addLocalMonitorForEvents(
      matching: [
        .keyDown, .leftMouseDown, .mouseMoved, .leftMouseDragged, .rightMouseDragged,
        .otherMouseDragged,
      ]
    ) { [weak self] event in
      guard let self, self.isPresenting, self.windowReference.value?.isKeyWindow == true else {
        return event
      }
      switch event.type {
      case .mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged:
        guard event.window === self.windowReference.value else { return event }
        self.noteActivity()
        return event
      case .leftMouseDown:
        guard event.window === self.windowReference.value else { return event }
        self.noteActivity()
        self.handle(.click)
        return nil
      case .keyDown:
        guard let input = Self.input(for: event) else { return event }
        let commandModified = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
          .contains(.command)
        guard self.handle(input, commandModified: commandModified) else { return event }
        self.noteActivity()
        return nil
      default:
        return event
      }
    }
  }

  private static func input(for event: NSEvent) -> WritingSlideShowInput? {
    switch event.keyCode {
    case 124: .rightArrow
    case 123: .leftArrow
    case 121: .pageDown
    case 116: .pageUp
    case 49: .space
    case 53: .escape
    default: nil
    }
  }

  @discardableResult
  private func handle(
    _ input: WritingSlideShowInput, commandModified: Bool = false
  ) -> Bool {
    guard let command = policy.command(for: input, commandModified: commandModified) else {
      return false
    }
    if navigation.apply(command) { requestExit() }
    return true
  }

  private func noteActivity() {
    guard let window = windowReference.value, window.isKeyWindow else { return }
    applyCursor(cursorState.reduce(.activity))
    cursorTask?.cancel()
    let delay = policy.cursorIdleDelay
    cursorTask = Task { @MainActor [weak self] in
      try? await Task.sleep(for: .seconds(delay))
      guard !Task.isCancelled, let self, self.isPresenting,
        self.windowReference.value?.isKeyWindow == true
      else { return }
      self.applyCursor(self.cursorState.reduce(.idleExpired))
    }
  }

  private func applyCursor(_ effect: WritingSlideShowCursorEffect) {
    switch effect {
    case .none: break
    case .hide: NSCursor.hide()
    case .show: NSCursor.unhide()
    }
  }

  private func finish(window: NSWindow) {
    let saved = snapshot
    let paneId = session?.paneId
    let viewportOffset = viewportOffsetState.takeRestoration()
    cleanUpSession()
    if !windowState.startedFullScreen, let saved {
      window.setFrame(saved.frame, display: true)
    }
    DispatchQueue.main.async { [weak window, weak responder = saved?.firstResponder] in
      if let window, let responder { window.makeFirstResponder(responder) }
      DispatchQueue.main.async {
        if let paneId, let viewportOffset {
          self.viewportRegistry.restore(viewportOffset, paneId: paneId)
        }
      }
    }
    snapshot = nil
    windowState = WritingSlideShowWindowState()
  }

  private func forceFinish() {
    let window = windowReference.value
    let paneId = session?.paneId
    let viewportOffset = viewportOffsetState.takeRestoration()
    cleanUpSession()
    windowState = WritingSlideShowWindowState()
    if let window, let saved = snapshot {
      window.acceptsMouseMovedEvents = saved.acceptsMouseMovedEvents
    }
    snapshot = nil
    DispatchQueue.main.async {
      if let paneId, let viewportOffset {
        self.viewportRegistry.restore(viewportOffset, paneId: paneId)
      }
    }
  }

  private func cleanUpSession() {
    cancelFullScreenEntryTimeout()
    cursorTask?.cancel()
    cursorTask = nil
    applyCursor(cursorState.reduce(.sessionEnded))
    if let inputMonitor {
      NSEvent.removeMonitor(inputMonitor)
      self.inputMonitor = nil
    }
    if let window = windowReference.value, let snapshot {
      window.acceptsMouseMovedEvents = snapshot.acceptsMouseMovedEvents
    }
    session = nil
    navigation = WritingSlideShowState(index: 0, slideCount: 0)
  }
}

struct MacWindowReader: NSViewRepresentable {
  let onResolve: @MainActor (NSWindow?) -> Void

  func makeNSView(context: Context) -> NSView {
    let view = NSView(frame: .zero)
    resolve(view)
    return view
  }

  func updateNSView(_ view: NSView, context: Context) { resolve(view) }

  private func resolve(_ view: NSView) {
    DispatchQueue.main.async { onResolve(view.window) }
  }
}

struct WritingPresentationPane: View {
  @Bindable var client: EngineClient
  @Environment(\.writingSlideShowAction) private var slideShow
  @Environment(\.writingSlideShowViewportRegistry) private var viewportRegistry
  let pane: Pane
  let headerDrag: PaneHeaderDragConfiguration?
  let companionLinked: Bool
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let layoutActionsDisabled: Bool
  @State private var selectedSlide: UInt32 = 0
  @State private var activeLine: UInt32 = 0
  @State private var targetLine: UInt32?
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
  private var slides: [WritingSlide] { writing?.structure?.slides ?? [] }
  private var writingLoadKey: String {
    writing?.loadIdentity ?? "missing"
  }

  var body: some View {
    GeometryReader { proxy in
      if let document = writing?.document, let file, file.viewport != nil,
        let plan = layoutPlan(size: proxy.size, document: document),
        case .presentation(let presentationPlan) = plan.surface
      {
        ScrollView(scrollAxes(plan), showsIndicators: !plan.overflowAxes.isEmpty) {
          plannedSurface(plan: plan, presentationPlan: presentationPlan, file: file)
            .forceEnclosingOverlayScrollers()
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDisabled(plan.overflowAxes.isEmpty)
        .frame(width: proxy.size.width, height: proxy.size.height)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
      } else {
        VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
          presentationHeader(document: writing?.document)
          ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(PaneLayoutContentMetrics.paneCardInset)
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
      }
    }
    .task(id: writingLoadKey) { await loadIfNeeded() }
    .onChange(of: showingOptions) { _, value in
      updateChrome(.menuChanged(value))
    }
    .alert("Close \(documentFileName)?", isPresented: $showingCloseConfirmation) {
      Button("Cancel", role: .cancel) {}
      Button("Close", role: .destructive) { client.closePane(pane.paneId) }
    } message: {
      Text("Unsaved changes will be lost.")
    }
  }

  private func layoutPlan(
    size: CGSize, document: WritingDocument
  ) -> WritingLayoutPlan? {
    let policy = WritingLayoutPolicy(
      floors: MacPaneLayoutMetrics.surfaceFloors,
      gaps: MacPaneLayoutMetrics.sizing.gaps)
    return try? policy.layout(
      WritingLayoutRequest(
        kind: .presentation,
        viewport: PaneLayoutSize(width: size.width, height: size.height),
        font: MacPaneLayoutMetrics.proseMetric(for: document.font)))
  }

  private func plannedSurface(
    plan: WritingLayoutPlan, presentationPlan: WritingPresentationLayoutPlan,
    file: EditorFileView
  ) -> some View {
    ZStack(alignment: .topLeading) {
      Color.clear
        .frame(width: plan.contentSize.width, height: plan.contentSize.height)
        .allowsHitTesting(false)
      MacTheme.face
        .writingFrame(plan.contentRect)
      presentationHeader(document: writing?.document)
        .writingFrame(plan.headerRect)
      if presentationPlan.navigation == .thumbnails {
        slideRail
          .writingFrame(presentationPlan.navigationRect)
          .accessibilityIdentifier("writing-presentation-slide-rail-\(pane.paneId)")
        MacTheme.well
          .writingFrame(
            PaneLayoutRect(
              origin: PaneLayoutPoint(
                x: presentationPlan.navigationRect.origin.x
                  + presentationPlan.navigationRect.size.width,
                y: presentationPlan.navigationRect.origin.y),
              size: PaneLayoutSize(
                width: WritingLayoutMetrics.standard.railDividerWidth,
                height: presentationPlan.navigationRect.size.height)))
      } else {
        slideNumberStrip
          .writingFrame(presentationPlan.navigationRect)
          .accessibilityIdentifier("writing-presentation-slide-numbers-\(pane.paneId)")
      }
      presentationStage(file)
        .writingFrame(presentationPlan.stageRect)
      footer
        .writingFrame(presentationPlan.footerRect)
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

  private func presentationHeader(document: WritingDocument?) -> some View {
    WritingChromeHeader(
      paneId: pane.paneId, title: documentFileName,
      metadata: writing?.structure.map {
        WritingDisplayText.count($0.slides.count, kind: .slide)
      },
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
        .focusable(false)
        .help("Presentation options")
        .accessibilityLabel("Presentation options menu")
        .accessibilityIdentifier("writing-options-\(pane.paneId)")
        .popover(isPresented: $showingOptions, arrowEdge: .bottom) {
          presentationOptions
        }
      })
  }

  private var presentationOptions: some View {
    let groups = MacPaneLayoutMetrics.chromePolicy.plan(kind: .presentation).menuGroups
    return VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
        if index > 0 { Divider().padding(.vertical, MacTheme.Space.x3) }
        ForEach(group, id: \.rawValue) { action in
          presentationOption(action)
        }
      }
    }
    .padding(MacTheme.Space.x8)
    .frame(width: PaneLayoutContentMetrics.writingOptionsMenuWidth)
    .background(MacTheme.face)
    .accessibilityElement(children: .contain)
    .accessibilityLabel("Presentation options menu")
  }

  @ViewBuilder private func presentationOption(_ action: WritingChromeAction) -> some View {
    switch action {
    case .present:
      optionButton(
        "Present", icon: "play.fill", disabled: slides.isEmpty,
        identifier: "writing-present-\(pane.paneId)"
      ) {
        showingOptions = false
        guard let fileId = writing?.document.file.fileId else { return }
        let viewportOffset = viewportRegistry?.capture(paneId: pane.paneId)
        let request = WritingSlideShowRequest(
          paneId: pane.paneId, fileId: fileId, initialIndex: Int(selectedSlide),
          slideCount: slides.count, viewportOffset: viewportOffset)
        Task { @MainActor in
          await Task.yield()
          slideShow(request)
        }
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
    case .font, .useFullWidth, .useReadingWidth, .focusMode, .ideaBoard, .scenes,
      .bold, .italic, .heading, .list, .quote, .link, .image:
      EmptyView()
    }
  }

  private func optionLabel(_ title: String, icon: String) -> some View {
    Label(title, systemImage: icon)
      .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      .foregroundStyle(MacTheme.ink)
      .frame(
        maxWidth: .infinity,
        minHeight: PaneLayoutContentMetrics.writingOptionsRowHeight,
        alignment: .leading
      )
      .contentShape(Rectangle())
  }

  private func optionButton(
    _ title: String, icon: String, role: ButtonRole? = nil,
    disabled: Bool = false, identifier: String, action: @escaping () -> Void
  ) -> some View {
    Button(role: role, action: action) { optionLabel(title, icon: icon) }
      .buttonStyle(.plain)
      .focusable(false)
      .disabled(disabled)
      .accessibilityIdentifier(identifier)
  }

  private func scrollAxes(_ plan: WritingLayoutPlan) -> Axis.Set {
    var axes: Axis.Set = []
    if plan.overflowAxes.contains(.horizontal) { axes.insert(.horizontal) }
    if plan.overflowAxes.contains(.vertical) { axes.insert(.vertical) }
    return axes
  }

  private var slideRail: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      HStack {
        Text("SLIDES")
          .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
        Spacer()
        Text("\(slides.count)")
          .font(MacTheme.mono(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
      }
      ScrollView {
        LazyVStack(spacing: MacTheme.Space.x10) {
          ForEach(slides, id: \.index) { slide in
            WritingSlideThumbnail(
              client: client, fileId: writing?.document.file.fileId, slide: slide,
              selected: slide.index == selectedSlide
            ) {
              select(slide)
            }
          }
        }
        .padding(MacTheme.Space.x4)
        .forceEnclosingOverlayScrollers()
      }
    }
    .padding(MacTheme.Space.x12).background(MacTheme.face)
  }

  private var slideNumberStrip: some View {
    ScrollView(.horizontal) {
      HStack(spacing: MacTheme.Space.x6) {
        ForEach(slides, id: \.index) { slide in
          Button {
            select(slide)
          } label: {
            Text("\(identifier: slide.index + 1)")
              .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
              .foregroundStyle(
                slide.index == selectedSlide ? MacTheme.onAccent : MacTheme.secondary
              )
              .frame(width: 22, height: 22)
              .background(
                slide.index == selectedSlide ? MacTheme.accent : MacTheme.well, in: Circle())
          }
          .buttonStyle(.plain)
          .accessibilityLabel("Slide \(identifier: slide.index + 1)")
          .accessibilityIdentifier(
            "writing-presentation-slide-number-\(pane.paneId)-\(slide.index)")
        }
      }
      .fixedSize(horizontal: true, vertical: false)
      .padding(.horizontal, MacTheme.Space.x10)
      .forceEnclosingOverlayScrollers()
    }
    .scrollIndicators(.hidden)
    .background(MacTheme.face)
  }

  private func presentationStage(_ file: EditorFileView) -> some View {
    WritingPresentationViewport(
      client: client, view: file, activeLine: $activeLine, targetLine: $targetLine,
      paneId: pane.paneId, viewportRegistry: viewportRegistry,
      onEditingActivity: { updateChrome(.edited) }
    )
    .background(MacTheme.face)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    .overlay {
      RoundedRectangle(cornerRadius: MacTheme.Radius.control)
        .stroke(MacTheme.well, lineWidth: 1)
    }
    .accessibilityIdentifier("writing-presentation-stage-\(pane.paneId)")
    .onChange(of: activeLine) { _, line in
      if let slide = slides.first(where: { line >= $0.startLine && line <= $0.endLine }) {
        selectedSlide = slide.index
      }
    }
  }

  private var footer: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "info.circle")
      Text("Title → appears on slide · regular text → your notes · --- → new slide")
      Spacer()
      Text("Markdown")
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.micro))
    .foregroundStyle(MacTheme.muted)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.presentationFooterHeight).background(MacTheme.face)
  }

  private var documentFileName: String {
    guard let path = writing?.document.file.path else { return pane.name }
    return URL(fileURLWithPath: path).lastPathComponent
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
    switch MacPaneLayoutMetrics.chromePolicy.closeDisposition(
      fileName: documentFileName, dirty: file?.file.dirty == true)
    {
    case .closeImmediately:
      client.closePane(pane.paneId)
    case .confirm:
      showingCloseConfirmation = true
    }
  }

  private func select(_ slide: WritingSlide) {
    selectedSlide = slide.index
    targetLine = slide.startLine
    guard let fileId = writing?.document.file.fileId,
      let viewport = file?.viewport,
      slide.startLine < viewport.fromLine || slide.startLine >= viewport.toLine
    else { return }
    Task { @MainActor in
      do { try await client.loadEditorViewport(fileId: fileId, fromLine: slide.startLine) } catch {
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

private struct WritingSlideThumbnail: View {
  @Bindable var client: EngineClient
  let fileId: String?
  let slide: WritingSlide
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
        HStack(alignment: .top) {
          Text("\(identifier: slide.index + 1)")
            .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
            .foregroundStyle(selected ? MacTheme.onAccent : MacTheme.secondary)
            .frame(width: 22, height: 22)
            .background(selected ? MacTheme.accent : MacTheme.well, in: Circle())
          Spacer()
          if !slide.imagePaths.isEmpty {
            Image(systemName: "photo").foregroundStyle(MacTheme.secondary)
          }
        }
        Text(WritingDisplayText.slideTitle(slide.heading))
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .foregroundStyle(MacTheme.ink).lineLimit(2)
        if let asset = slide.imagePaths.first, let fileId {
          WritingAssetImage(client: client, fileId: fileId, asset: asset)
            .frame(maxWidth: .infinity, minHeight: 34, maxHeight: 44)
        } else {
          Text(slide.notes.isEmpty ? "No speaker notes" : slide.notes)
            .font(MacTheme.chrome(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.muted).lineLimit(2)
        }
      }
      .padding(MacTheme.Space.x10)
      .frame(maxWidth: .infinity, minHeight: 100, alignment: .topLeading)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
      .overlay {
        RoundedRectangle(cornerRadius: MacTheme.Radius.control)
          .stroke(selected ? MacTheme.accent : MacTheme.well, lineWidth: selected ? 2 : 1)
      }
    }
    .buttonStyle(.plain)
  }
}

struct WritingSlideShowSurface: View {
  @Bindable var client: EngineClient
  let coordinator: MacWritingSlideShowCoordinator
  let session: WritingSlideShowSession

  private var writing: WritingDocumentView? {
    client.writingDocuments.values.first {
      $0.document.file.paneId == session.paneId && $0.document.file.fileId == session.fileId
    }
  }
  private var slides: [WritingSlide] { writing?.structure?.slides ?? [] }
  private var paneExists: Bool { client.panes.contains { $0.paneId == session.paneId } }
  private var reconcileKey: String {
    "\(client.connected):\(paneExists):\(slides.count)"
  }

  var body: some View {
    GeometryReader { proxy in
      let stage = WritingSlideShowPolicy.standard.fittedStage(
        in: PaneLayoutSize(width: proxy.size.width, height: proxy.size.height))
      ZStack(alignment: .topLeading) {
        MacTheme.presentationLetterbox
        if let slide = slides[safe: coordinator.navigation.index] {
          WritingPresentedSlide(client: client, fileId: session.fileId, slide: slide)
            .writingFrame(stage)
        }
      }
      .frame(width: proxy.size.width, height: proxy.size.height)
      .contentShape(Rectangle())
    }
    .ignoresSafeArea()
    .accessibilityIdentifier("writing-presentation-full-screen")
    .onAppear { reconcile() }
    .onChange(of: reconcileKey) { _, _ in reconcile() }
  }

  private func reconcile() {
    coordinator.reconcile(
      paneExists: paneExists, connected: client.connected, slideCount: slides.count)
  }
}

private struct WritingPresentedSlide: View {
  @Bindable var client: EngineClient
  let fileId: String
  let slide: WritingSlide

  var body: some View {
    GeometryReader { proxy in
      let metrics = WritingSlideShowPolicy.standard.slideSizing.metrics(
        in: PaneLayoutSize(width: proxy.size.width, height: proxy.size.height))
      VStack(spacing: metrics.contentSpacing) {
        Text(WritingDisplayText.slideTitle(slide.heading))
          .font(MacTheme.prose(.sans, metrics.titleSize, weight: .bold))
          .foregroundStyle(MacTheme.ink)
          .multilineTextAlignment(.center)
          .lineLimit(3)
          .minimumScaleFactor(0.55)
        if !slide.imagePaths.isEmpty {
          VStack(spacing: metrics.contentSpacing) {
            ForEach(slide.imagePaths, id: \.self) { asset in
              WritingAssetImage(client: client, fileId: fileId, asset: asset)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
          }
          .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
      }
      .padding(metrics.padding)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(MacTheme.face)
    }
    .accessibilityElement(children: .contain)
    .accessibilityIdentifier("writing-presentation-presented-slide")
  }
}

private struct WritingPresentationViewport: NSViewRepresentable {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var activeLine: UInt32
  @Binding var targetLine: UInt32?
  let paneId: String
  let viewportRegistry: WritingPresentationViewportRegistry?
  let onEditingActivity: () -> Void

  func makeCoordinator() -> Coordinator { Coordinator(self) }
  func makeNSView(context: Context) -> NSScrollView {
    let scrollView = context.coordinator.makeScrollView()
    viewportRegistry?.attach(scrollView, paneId: paneId)
    return scrollView
  }
  func updateNSView(_ scrollView: NSScrollView, context: Context) {
    context.coordinator.parent = self
    viewportRegistry?.attach(scrollView, paneId: paneId)
    context.coordinator.update()
  }

  static func dismantleNSView(_ scrollView: NSScrollView, coordinator: Coordinator) {
    coordinator.parent.viewportRegistry?.detach(scrollView, paneId: coordinator.parent.paneId)
  }

  @MainActor final class Coordinator: NSObject, NSTextViewDelegate {
    var parent: WritingPresentationViewport
    private let textView = WritingImagePasteTextView()
    private var source = ""
    private var firstLine: UInt32 = 0
    private var revision: String?
    private var renderedTheme: TerminalColorTheme?
    private var applying = false

    init(_ parent: WritingPresentationViewport) { self.parent = parent }

    func makeScrollView() -> NSScrollView {
      let scroll = NSScrollView()
      scroll.drawsBackground = true
      scroll.backgroundColor = NSColor(MacTheme.face)
      scroll.hasVerticalScroller = true
      scroll.autohidesScrollers = true
      scroll.scrollerStyle = .overlay
      scroll.documentView = textView
      configure()
      return scroll
    }

    func update() {
      guard let viewport = parent.view.viewport else { return }
      applyTheme()
      firstLine = viewport.fromLine
      let next = viewport.lines.map(\.text).joined(separator: "\n")
      let nextRevision = "\(viewport.file.revision):\(viewport.fromLine):\(viewport.toLine)"
      if nextRevision != revision || renderedTheme != SoyehtThemeStore.shared.activeTheme {
        source = next
        revision = nextRevision
        renderedTheme = SoyehtThemeStore.shared.activeTheme
        render(selection: valid(textView.selectedRange()))
      }
      if let line = parent.targetLine, line >= viewport.fromLine, line < viewport.toLine {
        scroll(to: line)
        parent.targetLine = nil
      }
    }

    private func applyTheme() {
      textView.enclosingScrollView?.backgroundColor = NSColor(MacTheme.face)
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
    }

    func textViewDidChangeSelection(_ notification: Notification) {
      guard !applying, let position = position(textView.selectedRange().location) else { return }
      parent.activeLine = position.line
    }

    func textView(
      _ textView: NSTextView, shouldChangeTextIn range: NSRange, replacementString: String?
    ) -> Bool {
      let replacement = replacementString ?? ""
      let raw = source as NSString
      guard NSMaxRange(range) <= raw.length,
        let start = position(range.location), let end = position(NSMaxRange(range))
      else { return false }
      parent.onEditingActivity()
      source = raw.replacingCharacters(in: range, with: replacement)
      let cursor = range.location + replacement.utf16.count
      parent.client.enqueueEditorEdits(
        fileId: parent.view.file.fileId,
        edits: [TextEdit(range: .init(start: start, end: end), text: replacement)])
      parent.activeLine = start.line &+ UInt32(clamping: replacement.filter { $0 == "\n" }.count)
      render(selection: NSRange(location: cursor, length: 0))
      return false
    }

    private func render(selection: NSRange) {
      applying = true
      textView.textStorage?.setAttributedString(WritingPresentationStyle.attributed(source))
      textView.setSelectedRange(valid(selection))
      textView.needsDisplay = true
      applying = false
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

    private func valid(_ range: NSRange) -> NSRange {
      let length = source.utf16.count
      guard range.location != NSNotFound, range.location <= length else { return .init() }
      return NSRange(location: range.location, length: min(range.length, length - range.location))
    }

    private func configure() {
      textView.delegate = self
      textView.isRichText = false
      textView.importsGraphics = false
      textView.allowsUndo = true
      textView.usesFindPanel = true
      textView.drawsBackground = true
      textView.backgroundColor = NSColor(MacTheme.face)
      textView.insertionPointColor = NSColor(MacTheme.accent)
      textView.selectedTextAttributes = [.backgroundColor: NSColor(MacTheme.selection)]
      textView.textContainerInset = NSSize(width: 48, height: 36)
      textView.minSize = .zero
      textView.maxSize = NSSize(
        width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
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

@MainActor private enum WritingPresentationStyle {
  static func attributed(_ source: String) -> NSAttributedString {
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = 23
    paragraph.maximumLineHeight = 23
    let result = NSMutableAttributedString(
      string: source,
      attributes: [
        .font: MacTheme.monoNSFont(12), .foregroundColor: NSColor(MacTheme.ink),
        .paragraphStyle: paragraph,
      ])
    var location = 0
    for line in source.components(separatedBy: "\n") {
      let range = NSRange(location: location, length: line.utf16.count)
      if line.hasPrefix("#") {
        result.addAttributes(
          [.foregroundColor: NSColor(MacTheme.accent), .font: MacTheme.monoNSFont(12, bold: true)],
          range: range)
      } else if line.trimmingCharacters(in: .whitespaces) == "---" {
        result.addAttribute(.foregroundColor, value: NSColor(MacTheme.muted), range: range)
      } else if line.trimmingCharacters(in: .whitespaces).hasPrefix("![") {
        result.addAttributes(
          [.foregroundColor: NSColor(MacTheme.success), .backgroundColor: NSColor(MacTheme.well)],
          range: range)
      }
      location += line.utf16.count + 1
    }
    return result
  }
}

extension Collection {
  fileprivate subscript(safe index: Index) -> Element? {
    indices.contains(index) ? self[index] : nil
  }
}

private struct WritingAssetImage: View {
  @Bindable var client: EngineClient
  let fileId: String
  let asset: String
  @State private var image: NSImage?

  var body: some View {
    Group {
      if let image {
        Image(nsImage: image).resizable().scaledToFit()
      } else {
        ProgressView().controlSize(.small)
      }
    }
    .task(id: "\(fileId):\(asset)") {
      do {
        let bytes = try await client.readWritingAsset(fileId: fileId, asset: asset)
        guard let decoded = NSImage(data: bytes) else { throw ClientFailure.invalidFrame }
        image = decoded
      } catch { client.problem = error.localizedDescription }
    }
  }
}

@MainActor final class WritingImagePasteTextView: NSTextView {
  var imageHandler: ((NSPasteboard) -> Bool)?
  var writingFormatShortcutHandler: ((WritingFormatKind) -> Void)?

  override func performKeyEquivalent(with event: NSEvent) -> Bool {
    let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
    let conflictingModifiers: NSEvent.ModifierFlags = [.control, .option, .shift]
    guard flags.contains(.command), flags.intersection(conflictingModifiers).isEmpty,
      let kind = WritingChromePolicy.formatShortcut(
        key: event.charactersIgnoringModifiers ?? "", command: true),
      let writingFormatShortcutHandler
    else { return super.performKeyEquivalent(with: event) }
    writingFormatShortcutHandler(kind)
    return true
  }

  override func paste(_ sender: Any?) {
    if imageHandler?(NSPasteboard.general) == true { return }
    super.paste(sender)
  }

  override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
    if imageHandler?(sender.draggingPasteboard) == true { return true }
    return super.performDragOperation(sender)
  }
}

enum WritingImageAttachment {
  static func make(from image: NSImage) throws -> WebHostAttachment {
    let bytes = try boundedPNG(image)
    let reference = AttachmentRef(
      attachmentId: UUID().uuidString.lowercased(), mimeType: "image/png", name: "image.png",
      bytes: String(bytes.count))
    return WebHostAttachment(reference: reference, bytes: bytes)
  }

  private static func boundedPNG(_ image: NSImage) throws -> Data {
    var candidate = image
    for _ in 0..<8 {
      guard let tiff = candidate.tiffRepresentation,
        let bitmap = NSBitmapImageRep(data: tiff),
        let png = bitmap.representation(using: .png, properties: [:])
      else { throw ClientFailure.editorCommand("The image could not be encoded.") }
      if png.count <= ProtocolLimits.writingImageBytes { return png }
      let ratio = sqrt(Double(ProtocolLimits.writingImageBytes) / Double(png.count)) * 0.85
      candidate = try resized(candidate, ratio: min(0.9, ratio))
    }
    throw ClientFailure.editorCommand("The image exceeds its byte limit.")
  }

  private static func resized(_ image: NSImage, ratio: Double) throws -> NSImage {
    let width = max(1, Int(floor(image.size.width * ratio)))
    let height = max(1, Int(floor(image.size.height * ratio)))
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)
    else { throw ClientFailure.editorCommand("The image could not be resized.") }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    image.draw(
      in: NSRect(x: 0, y: 0, width: width, height: height), from: .zero,
      operation: .copy, fraction: 1)
    let resized = NSImage(size: NSSize(width: width, height: height))
    resized.addRepresentation(bitmap)
    return resized
  }
}
