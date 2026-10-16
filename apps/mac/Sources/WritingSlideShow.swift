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
  @ObservationIgnored private var cursorRectsDisabled = false
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
    disableCursorRects(in: window)
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
    guard let window, owns(window), isPresenting else { return }
    disableCursorRects(in: window)
    noteActivity()
  }

  func windowDidResignKey(_ window: NSWindow?) {
    guard owns(window), isPresenting else { return }
    enableCursorRects(in: window)
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
        NSCursor.arrow.set()
        return nil
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

  private func disableCursorRects(in window: NSWindow) {
    guard !cursorRectsDisabled else {
      NSCursor.arrow.set()
      return
    }
    window.disableCursorRects()
    cursorRectsDisabled = true
    NSCursor.arrow.set()
  }

  private func enableCursorRects(in window: NSWindow?) {
    guard cursorRectsDisabled else { return }
    window?.enableCursorRects()
    cursorRectsDisabled = false
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
    enableCursorRects(in: windowReference.value)
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

struct WritingSlideShowHostModifier: ViewModifier {
  @Bindable var client: EngineClient
  let coordinator: MacWritingSlideShowCoordinator

  func body(content: Content) -> some View {
    content
      .overlay {
        if let session = coordinator.session {
          WritingSlideShowSurface(client: client, coordinator: coordinator, session: session)
            .zIndex(100)
        }
      }
      .background(MacWindowReader { coordinator.attach(window: $0) })
      .environment(
        \.writingSlideShowAction,
        WritingSlideShowAction { request in coordinator.present(request) }
      )
      .environment(\.writingSlideShowViewportRegistry, coordinator.viewportRegistry)
      .modifier(WritingSlideShowWindowEventsModifier(coordinator: coordinator))
      .onChange(of: client.panes.map(\.paneId)) { _, paneIds in
        guard let session = coordinator.session else { return }
        coordinator.reconcile(
          paneExists: paneIds.contains(session.paneId), connected: client.connected,
          slideCount: coordinator.navigation.slideCount)
      }
      .onChange(of: client.connected) { _, connected in
        guard let session = coordinator.session else { return }
        coordinator.reconcile(
          paneExists: client.panes.contains { $0.paneId == session.paneId },
          connected: connected, slideCount: coordinator.navigation.slideCount)
      }
  }
}

private struct WritingSlideShowWindowEventsModifier: ViewModifier {
  let coordinator: MacWritingSlideShowCoordinator

  func body(content: Content) -> some View {
    content
      .onReceive(
        NotificationCenter.default.publisher(for: NSWindow.didEnterFullScreenNotification)
      ) {
        coordinator.windowDidEnterFullScreen($0.object as? NSWindow)
      }
      .onReceive(
        NotificationCenter.default.publisher(for: NSWindow.didExitFullScreenNotification)
      ) {
        coordinator.windowDidExitFullScreen($0.object as? NSWindow)
      }
      .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) {
        coordinator.windowDidBecomeKey($0.object as? NSWindow)
      }
      .onReceive(NotificationCenter.default.publisher(for: NSWindow.didResignKeyNotification)) {
        coordinator.windowDidResignKey($0.object as? NSWindow)
      }
      .onReceive(NotificationCenter.default.publisher(for: NSWindow.willCloseNotification)) {
        coordinator.windowWillClose($0.object as? NSWindow)
      }
  }
}
