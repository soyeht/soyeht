import Foundation
import SoyehtClient
import SoyehtProtocol
import WebKit

@MainActor final class MacWebHost: WebHostPort {
  private weak var client: EngineClient?
  private var sessions: [String: MacWebSession] = [:]

  func bind(_ client: EngineClient) { self.client = client }

  func session(for pane: Pane) -> MacWebSession {
    if let existing = sessions[pane.paneId], existing.incognito == (pane.incognito == true) {
      existing.advanceMinimumGeneration(pane.generation)
      existing.loadIfNeeded(pane.url)
      return existing
    }
    let session = MacWebSession(
      paneId: pane.paneId, incognito: pane.incognito == true,
      navigationGeneration: pane.generation,
      onNavigated: { [weak self] generation, url, title in
        await self?.reportNavigation(
          paneId: pane.paneId, generation: generation, url: url, title: title)
      })
    sessions[pane.paneId] = session
    session.loadIfNeeded(pane.url)
    return session
  }

  func reconcile(_ panes: [Pane]) {
    let live = Set(panes.lazy.filter { $0.mode == .web }.map(\.paneId))
    for paneId in Array(sessions.keys) where !live.contains(paneId) {
      sessions.removeValue(forKey: paneId)?.stop()
    }
  }

  func resynchronize() {
    for session in sessions.values { session.reportCurrentNavigation() }
  }

  func execute(_ command: WebCommandParams) async throws -> WebCommandResult {
    guard
      let pane = client?.panes.first(where: {
        $0.paneId == command.paneId && $0.mode == .web
      })
    else {
      throw ClientFailure.webHost(.internal_error, "The web pane is unavailable.")
    }
    return try await session(for: pane).execute(command)
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
}

@MainActor final class MacWebSession: NSObject, WKNavigationDelegate {
  let paneId: String
  let incognito: Bool
  let webView: WKWebView
  private let onNavigated: (UInt64, String, String) async -> Void
  private var requestedURL: URL?
  private var navigationGeneration: UInt64 = 0

  init(
    paneId: String, incognito: Bool, navigationGeneration: UInt32,
    onNavigated: @escaping (UInt64, String, String) async -> Void
  ) {
    self.paneId = paneId
    self.incognito = incognito
    self.navigationGeneration = UInt64(navigationGeneration)
    self.onNavigated = onNavigated
    let configuration = WKWebViewConfiguration()
    configuration.websiteDataStore = incognito ? .nonPersistent() : .default()
    webView = WKWebView(frame: .zero, configuration: configuration)
    super.init()
    webView.navigationDelegate = self
    webView.allowsMagnification = true
    webView.isInspectable = true
  }

  var canGoBack: Bool { webView.canGoBack }

  var canGoForward: Bool { webView.canGoForward }

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

  func execute(_ command: WebCommandParams) async throws -> WebCommandResult {
    guard command.paneId == paneId else {
      throw ClientFailure.webHost(.internal_error, "The web pane identity changed.")
    }
    guard let access = accessMode(command.args),
      access == "human" || (incognito && access == "incognito")
        || (!incognito && ["probe", "granted"].contains(access))
    else {
      throw ClientFailure.webHost(.internal_error, "The web command access is invalid.")
    }
    if access == "probe" {
      if await cookiesPresent(for: targetHost(command.args)) {
        return WebCommandResult(paneId: paneId, cookiesPresent: true)
      }
      var result = try perform(command)
      result.cookiesPresent = false
      return result
    }
    return try perform(command)
  }

  private func perform(_ command: WebCommandParams) throws -> WebCommandResult {
    guard command.command == .navigate else {
      throw ClientFailure.webHost(.internal_error, "The web command is not implemented.")
    }
    try navigate(command.args)
    return WebCommandResult(paneId: paneId, value: .object(["accepted": .bool(true)]))
  }

  func stop() {
    webView.stopLoading()
    webView.navigationDelegate = nil
  }

  func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation?) {
    navigationGeneration &+= 1
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

  private func allowedURL(_ raw: String) -> URL? {
    guard let components = URLComponents(string: raw),
      let scheme = components.scheme?.lowercased(), ["http", "https"].contains(scheme),
      components.host?.isEmpty == false
    else { return nil }
    return components.url
  }
}
