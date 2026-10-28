import AppKit
import Foundation
import Observation
import SoyehtClient
import SoyehtProtocol
import WebKit

/// What of a web pane decides its session, and whether it is on screen: a change to any of these
/// needs `MacWebHost.reconcile`.
struct WebPaneKey: Hashable {
  let paneId: String
  let incognito: Bool
  let url: String?
  let generation: UInt32
  let needsSession: Bool

  init(_ pane: Pane, needsSession: Bool) {
    self.needsSession = needsSession
    paneId = pane.paneId
    incognito = pane.incognito == true
    url = pane.url
    generation = pane.generation
  }
}

/// Owns one session per web pane. Sessions are created and advanced in `reconcile` (and for an
/// agent command in `execute`), never while a view renders: a view only reads `session(for:)`.
@MainActor @Observable final class MacWebHost: WebHostPort {
  @ObservationIgnored private weak var client: EngineClient?
  private var sessions: [String: MacWebSession] = [:]

  func bind(_ client: EngineClient) { self.client = client }

  /// The pane's session, once `reconcile` has made it.
  func session(for pane: Pane) -> MacWebSession? { sessions[pane.paneId] }

  /// Makes the sessions of the web panes on screen (`shownWorkspaceId` under `destination`),
  /// advances the ones it has, and ends those whose pane is gone. Another window may show another
  /// workspace, so a pane that is not on screen here keeps the session it has.
  func reconcile(_ panes: [Pane], shownWorkspaceId: String?, destination: WorkspaceDestination) {
    end(WebSessionLifecycle.ended(sessions: kinds(of: Set(sessions.keys)), panes: panes))
    let needed = WebSessionLifecycle.needed(
      panes: panes, shownWorkspaceId: shownWorkspaceId, destination: destination)
    for pane in panes where needed.contains(pane.paneId) { synchronize(pane) }
  }

  private func kinds(of paneIds: Set<String>) -> [String: Bool] {
    sessions.filter { paneIds.contains($0.key) }.mapValues(\.incognito)
  }

  private func end(_ paneIds: Set<String>) {
    for paneId in paneIds { sessions.removeValue(forKey: paneId)?.stop() }
  }

  /// Makes the session of a pane a view is showing, if `reconcile` has not: a net for a page
  /// that draws a web pane without `WebSessionLifecycle.needed` knowing it. Called when the view
  /// appears or the pane changes, never from a body.
  func ensureSession(for pane: Pane) { synchronize(pane) }

  @discardableResult
  private func synchronize(_ pane: Pane) -> MacWebSession {
    if let existing = sessions[pane.paneId] {
      existing.advanceMinimumGeneration(pane.generation)
      existing.loadIfNeeded(pane.url)
      return existing
    }
    let session = MacWebSession(
      paneId: pane.paneId, incognito: pane.incognito == true,
      navigationGeneration: pane.generation, viewport: pane.viewport,
      onNavigated: { [weak self] generation, url, title in
        await self?.reportNavigation(
          paneId: pane.paneId, generation: generation, url: url, title: title)
      },
      onConsoleCounts: { [weak self] warnings, errors in
        await self?.reportConsoleCounts(
          paneId: pane.paneId, warnings: warnings, errors: errors)
      })
    sessions[pane.paneId] = session
    session.loadIfNeeded(pane.url)
    return session
  }

  func resynchronize() {
    for session in sessions.values { session.reportCurrentNavigation() }
  }

  func execute(_ command: WebCommandParams) async throws -> WebHostOutput {
    guard
      let pane = client?.panes.first(where: {
        $0.paneId == command.paneId && $0.mode == .web
      })
    else {
      throw ClientFailure.webHost(.internal_error, "The web pane is unavailable.")
    }
    if humanCommand(command.args) { client?.select(command.paneId) }
    end(WebSessionLifecycle.ended(sessions: kinds(of: [pane.paneId]), panes: [pane]))
    return try await synchronize(pane).execute(command)
  }

  private func humanCommand(_ args: JsonValue) -> Bool {
    guard case .object(let values) = args,
      case .object(let access)? = values["_soyehtAccess"],
      case .string("human")? = access["session"]
    else { return false }
    return true
  }

  private func reportNavigation(
    paneId: String, generation: UInt64, url: String, title: String
  ) async {
    guard let client else { return }
    do {
      try await client.reportWebNavigation(
        paneId: paneId, navigationGeneration: String(generation), url: url, title: title,
        idempotencyKey: UUID().uuidString)
    } catch {
      return
    }
  }

  private func reportConsoleCounts(paneId: String, warnings: UInt32, errors: UInt32) async {
    guard let client else { return }
    do {
      try await client.reportWebConsoleCounts(
        paneId: paneId, warnings: warnings, errors: errors,
        idempotencyKey: UUID().uuidString)
    } catch {
      return
    }
  }
}

@MainActor @Observable final class MacWebSession: NSObject, WKNavigationDelegate {
  let paneId: String
  let incognito: Bool
  @ObservationIgnored
  let webView: WKWebView
  @ObservationIgnored
  private let onNavigated: (UInt64, String, String) async -> Void
  private var requestedURL: URL?
  private var navigationGeneration: UInt64 = 0
  private(set) var viewport: WebViewport?
  @ObservationIgnored
  private let structuredActions: WebStructuredActions
  @ObservationIgnored
  private let pageObservation: WebPageObservation
  @ObservationIgnored
  private var inputMonitor: Any?
  let elementPicker: WebElementPicker
  @ObservationIgnored
  private let reader: WebPageReader
  private var control = WebControlHandoff()

  init(
    paneId: String, incognito: Bool, navigationGeneration: UInt32,
    viewport: WebViewport?,
    onNavigated: @escaping (UInt64, String, String) async -> Void,
    onConsoleCounts: @escaping (UInt32, UInt32) async -> Void
  ) {
    self.paneId = paneId
    self.incognito = incognito
    self.navigationGeneration = UInt64(navigationGeneration)
    self.viewport = viewport
    self.onNavigated = onNavigated
    let configuration = WKWebViewConfiguration()
    configuration.websiteDataStore = incognito ? .nonPersistent() : .default()
    configuration.applicationNameForUserAgent = SafariIdentity.applicationName
    let observation = WebPageObservation(onCountsChanged: onConsoleCounts)
    let view = WKWebView(frame: .zero, configuration: configuration)
    webView = view
    structuredActions = WebStructuredActions(webView: view)
    pageObservation = observation
    elementPicker = WebElementPicker(webView: view)
    reader = WebPageReader(webView: view)
    super.init()
    inputMonitor = NSEvent.addLocalMonitorForEvents(
      matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown, .keyDown, .scrollWheel]
    ) { [weak self, weak view] event in
      self?.observeHumanInput(event, in: view)
      return event
    }
    pageObservation.bind(view)
    webView.navigationDelegate = self
    webView.allowsMagnification = true
    webView.isInspectable = true
  }

  var canGoBack: Bool { webView.canGoBack }

  var canGoForward: Bool { webView.canGoForward }

  var zoomPercent: UInt32 { UInt32((webView.pageZoom * 100).rounded()) }

  var humanHasControl: Bool { control.humanHasControl }

  var agentCommandRunning: Bool { control.agentCommandRunning }

  func noteHumanInput() { control.noteHumanInput() }

  func handBack() { control.handBack() }

  func consoleEntries() -> [WebConsoleEntry] { pageObservation.consoleEntries() }

  func networkEntries() -> [WebNetworkEntry] { pageObservation.networkEntries() }

  func setZoomPercent(_ percent: UInt32) {
    let bounded = min(200, max(50, percent))
    webView.pageZoom = Double(bounded) / 100
    if let viewport {
      self.viewport = WebViewport(
        width: viewport.width, height: viewport.height, preset: viewport.preset,
        zoomPercent: bounded)
    }
  }

  func find(_ query: String, backwards: Bool) async -> Bool {
    let bounded = String(query.prefix(4_096))
    guard !bounded.isEmpty else { return false }
    let configuration = WKFindConfiguration()
    configuration.backwards = backwards
    configuration.wraps = true
    return await withCheckedContinuation { continuation in
      webView.find(bounded, configuration: configuration) { result in
        continuation.resume(returning: result.matchFound)
      }
    }
  }

  func humanScreenshot(fullPage: Bool) async throws -> Data {
    guard fullPage else { return try await WebScreenshotCapture.capture(webView: webView).bytes }
    return try await WebScreenshotCapture.captureFullPage(
      reader: reader,
      deadline: WebPageReader.deadline(forTimeoutMs: UInt32(ProtocolLimits.webWaitTimeoutMs)),
      between: {}
    ).attachment.bytes
  }

  func loadIfNeeded(_ rawURL: String?) {
    guard webView.url == nil, requestedURL == nil, let rawURL,
      let url = allowedURL(rawURL)
    else { return }
    requestedURL = url
    webView.load(URLRequest(url: url))
  }

  func advanceMinimumGeneration(_ generation: UInt32) {
    navigationGeneration = max(navigationGeneration, UInt64(generation))
  }

  func execute(_ command: WebCommandParams) async throws -> WebHostOutput {
    guard command.paneId == paneId else {
      throw ClientFailure.webHost(.internal_error, "The web pane identity changed.")
    }
    guard let access = accessMode(command.args),
      access == "human" || (incognito && access == "incognito")
        || (!incognito && ["probe", "granted"].contains(access))
    else {
      throw ClientFailure.webHost(.internal_error, "The web command access is invalid.")
    }
    let agentCommand = access != "human"
    if agentCommand && !control.beginAgentCommand() {
      throw ClientFailure.webHost(
        .human_took_over, "A person has control of this web pane. Wait for hand-back.")
    }
    defer {
      if agentCommand { control.finishAgentCommand() }
    }
    if access == "probe" {
      if await cookiesPresent(for: targetHost(command.args)) {
        return WebHostOutput(result: WebCommandResult(paneId: paneId, cookiesPresent: true))
      }
      var output = try await perform(command)
      output.result.cookiesPresent = false
      return output
    }
    return try await perform(command)
  }

  private func perform(_ command: WebCommandParams) async throws -> WebHostOutput {
    switch command.command {
    case .navigate:
      try navigate(command.args)
      return output(.object(["accepted": .bool(true)]))
    case .resize:
      let resized = try resize(command.args)
      return output(viewportValue(resized))
    case .snapshot:
      var value = try await structuredActions.snapshot(generation: navigationGeneration)
      guard boolean(command.args, "screenshot") == true else { return output(value) }
      let attachment = try await WebScreenshotCapture.capture(webView: webView)
      value = setting(value, "screenshot", to: try encoded(attachment.reference))
      return output(value, attachments: [attachment])
    case .screenshot:
      guard boolean(command.args, "fullPage") == true else {
        let attachment = try await WebScreenshotCapture.capture(webView: webView)
        return output(
          .object(["image": try encoded(attachment.reference)]), attachments: [attachment])
      }
      let full = try await WebScreenshotCapture.captureFullPage(
        reader: reader, deadline: WebPageReader.deadline(forTimeoutMs: command.timeoutMs),
        between: { try self.requirePersonAbsent() })
      return output(
        .object([
          "image": try encoded(full.attachment.reference), "endOfPage": .bool(full.endOfPage),
        ]), attachments: [full.attachment])
    case .read:
      return try await read(command)
    case .click, .type, .press, .hover, .select, .scroll:
      try await structuredActions.perform(
        command.command, args: command.args, generation: navigationGeneration)
      return output(.object(["completed": .bool(true)]))
    case .evaluate:
      let value = try await structuredActions.evaluate(command.args)
      return output(.object(["value": .string(value)]))
    case .wait:
      let matched = try await pageObservation.wait(
        args: command.args, timeoutMs: command.timeoutMs)
      return output(.object(["matched": .bool(matched)]))
    case .console:
      return output(try pageObservation.consolePage(since: value(command.args, "since")))
    case .network:
      return output(try pageObservation.networkPage(since: value(command.args, "since")))
    default:
      throw ClientFailure.webHost(.internal_error, "The web command is not implemented.")
    }
  }

  /// A person's input in the pane ends an agent command at its next step.
  private func requirePersonAbsent() throws {
    if control.humanHasControl {
      throw ClientFailure.webHost(
        .human_took_over, "A person has control of this web pane. Wait for hand-back.")
    }
  }

  private func read(_ command: WebCommandParams) async throws -> WebHostOutput {
    let screens =
      unsigned(command.args, "screens").map(Int.init) ?? ProtocolLimits.webReadScreensDefault
    let capture = try await reader.read(
      screens: screens, collectsContent: true,
      deadline: WebPageReader.deadline(forTimeoutMs: command.timeoutMs),
      between: { try self.requirePersonAbsent() })
    let attachment = try WebScreenshotCapture.attachment(for: capture.image)
    let value = try WebReadAssembly.value(
      text: capture.text, links: capture.links, screens: capture.screens,
      endOfPage: capture.endOfPage, truncated: capture.truncated,
      generation: navigationGeneration, title: webView.title ?? "",
      url: webView.url?.absoluteString ?? "", image: attachment.reference)
    return output(value, attachments: [attachment])
  }

  private func output(
    _ value: JsonValue, attachments: [WebHostAttachment] = []
  ) -> WebHostOutput {
    WebHostOutput(result: WebCommandResult(paneId: paneId, value: value), attachments: attachments)
  }

  private func encoded<T: Encodable>(_ value: T) throws -> JsonValue {
    try JSONDecoder().decode(JsonValue.self, from: JSONEncoder().encode(value))
  }

  private func setting(_ value: JsonValue, _ key: String, to replacement: JsonValue) -> JsonValue {
    guard case .object(var fields) = value else { return value }
    fields[key] = replacement
    return .object(fields)
  }

  func stop() {
    if let inputMonitor { NSEvent.removeMonitor(inputMonitor) }
    inputMonitor = nil
    elementPicker.navigationStarted()
    webView.stopLoading()
    webView.navigationDelegate = nil
    pageObservation.stop()
  }

  func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation?) {
    navigationGeneration &+= 1
    structuredActions.navigationStarted()
    elementPicker.navigationStarted()
  }

  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation?) {
    reportCurrentNavigation()
  }

  func reportCurrentNavigation() {
    guard let url = webView.url?.absoluteString else { return }
    requestedURL = webView.url
    let generation = navigationGeneration
    let title = webView.title ?? ""
    Task { await onNavigated(generation, url, title) }
  }

  func webView(
    _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation?,
    withError error: any Error
  ) {
    requestedURL = nil
  }

  func webView(
    _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
  ) async -> WKNavigationActionPolicy {
    guard let url = navigationAction.request.url, allowedURL(url.absoluteString) != nil else {
      return .cancel
    }
    if navigationAction.targetFrame?.isMainFrame == true {
      pageObservation.navigationWillStart(to: url)
    }
    return .allow
  }

  private func navigate(_ args: JsonValue) throws {
    if let rawURL = value(args, "url"), let url = allowedURL(rawURL) {
      requestedURL = url
      webView.load(URLRequest(url: url))
      return
    }
    guard let rawAction = value(args, "action"),
      let action = WebNavigationAction(rawValue: rawAction)
    else {
      throw ClientFailure.webHost(.navigation_refused, "The navigation target is invalid.")
    }
    switch action {
    case .back where webView.canGoBack: webView.goBack()
    case .forward where webView.canGoForward: webView.goForward()
    case .reload: webView.reload()
    case .reload_ignoring_cache: webView.reloadFromOrigin()
    default:
      throw ClientFailure.webHost(.navigation_refused, "The navigation action is unavailable.")
    }
  }

  private func resize(_ args: JsonValue) throws -> WebViewport {
    let preset = value(args, "preset").flatMap(WebViewportPreset.init(rawValue:)) ?? .responsive
    var size = try viewportSize(args, preset: preset)
    if boolean(args, "rotate") == true { size = (size.1, size.0) }
    let resized = WebViewport(
      width: size.0, height: size.1, preset: preset, zoomPercent: viewport?.zoomPercent ?? 100)
    viewport = resized
    return resized
  }

  private func viewportSize(_ args: JsonValue, preset: WebViewportPreset) throws -> (UInt32, UInt32)
  {
    let size: (UInt32, UInt32) =
      switch preset {
      case .iphone_se: (375, 667)
      case .iphone15_pro: (393, 852)
      case .ipad: (768, 1_024)
      case .responsive:
        try (
          unsigned(args, "width").requiredViewportValue(),
          unsigned(args, "height").requiredViewportValue()
        )
      }
    guard size.0 <= UInt32(ProtocolLimits.webExtentMax),
      size.1 <= UInt32(ProtocolLimits.webExtentMax)
    else {
      throw ClientFailure.webHost(.internal_error, "The web viewport is invalid.")
    }
    return size
  }

  private func viewportValue(_ viewport: WebViewport) -> JsonValue {
    .object([
      "viewport": .object([
        "width": .integer(Int64(viewport.width)),
        "height": .integer(Int64(viewport.height)),
        "preset": .string(viewport.preset.rawValue),
        "zoomPercent": .integer(Int64(viewport.zoomPercent)),
      ])
    ])
  }

  private func cookiesPresent(for host: String?) async -> Bool {
    guard let host else { return false }
    let cookies = await webView.configuration.websiteDataStore.httpCookieStore.allCookies()
    return cookies.contains { cookie in
      let domain = cookie.domain.lowercased().trimmingCharacters(
        in: CharacterSet(charactersIn: "."))
      let target = host.lowercased()
      return target == domain || target.hasSuffix(".\(domain)")
    }
  }

  private func targetHost(_ args: JsonValue) -> String? {
    if let rawURL = value(args, "url") { return allowedURL(rawURL)?.host }
    return webView.url?.host ?? requestedURL?.host
  }

  private func accessMode(_ args: JsonValue) -> String? {
    guard case .object(let values) = args,
      case .object(let access)? = values["_soyehtAccess"],
      case .string(let mode)? = access["session"]
    else { return nil }
    return mode
  }

  private func value(_ args: JsonValue, _ key: String) -> String? {
    guard case .object(let values) = args, case .string(let value)? = values[key] else {
      return nil
    }
    return value
  }

  private func unsigned(_ args: JsonValue, _ key: String) -> UInt32? {
    guard case .object(let values) = args, case .integer(let value)? = values[key] else {
      return nil
    }
    return UInt32(exactly: value)
  }

  private func boolean(_ args: JsonValue, _ key: String) -> Bool? {
    guard case .object(let values) = args, case .bool(let value)? = values[key] else {
      return nil
    }
    return value
  }

  private func allowedURL(_ raw: String) -> URL? {
    guard let components = URLComponents(string: raw),
      let scheme = components.scheme?.lowercased(), ["http", "https"].contains(scheme),
      components.host?.isEmpty == false
    else { return nil }
    return components.url
  }

  private func observeHumanInput(_ event: NSEvent, in view: WKWebView?) {
    guard let view, event.window === view.window else { return }
    if event.type == .keyDown {
      guard let responder = view.window?.firstResponder as? NSView,
        responder === view || responder.isDescendant(of: view)
      else { return }
      noteHumanInput()
      return
    }
    let point = view.convert(event.locationInWindow, from: nil)
    if view.bounds.contains(point) { noteHumanInput() }
  }
}

extension UInt32? {
  fileprivate func requiredViewportValue() throws -> UInt32 {
    guard let self, self > 0 else {
      throw ClientFailure.webHost(.internal_error, "The web viewport is invalid.")
    }
    return self
  }
}
