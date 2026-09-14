import Foundation
import SoyehtClient
import SoyehtProtocol
import WebKit

private struct PageObservationMessage: Decodable {
  var kind: String
  var level: String?
  var text: String?
  var source: String?
  var method: String?
  var url: String?
  var status: UInt32?
  var durationMs: UInt32?
  var bytes: String?
  var at: String
}

@MainActor final class WebPageObservation: NSObject, WKScriptMessageHandler {
  private weak var webView: WKWebView?
  private let handlerName = "soyeht_\(UUID().uuidString.replacingOccurrences(of: "-", with: ""))"
  private let onCountsChanged: (UInt32, UInt32) async -> Void
  private var buffer = WebObservationBuffer()
  private var reportTask: Task<Void, Never>?

  init(onCountsChanged: @escaping (UInt32, UInt32) async -> Void) {
    self.onCountsChanged = onCountsChanged
  }

  func install(in configuration: WKWebViewConfiguration) {
    let encodedName = "\"\(handlerName)\""
    let script = WKUserScript(
      source: Self.observerScript.replacingOccurrences(of: "__HANDLER__", with: encodedName),
      injectionTime: .atDocumentStart, forMainFrameOnly: true, in: .page)
    configuration.userContentController.add(self, contentWorld: .page, name: handlerName)
    configuration.userContentController.addUserScript(script)
  }

  func bind(_ webView: WKWebView) { self.webView = webView }

  func stop() {
    reportTask?.cancel()
    webView?.configuration.userContentController.removeScriptMessageHandler(
      forName: handlerName, contentWorld: .page)
  }

  func userContentController(
    _ userContentController: WKUserContentController, didReceive message: WKScriptMessage
  ) {
    guard let raw = message.body as? String, raw.utf8.count <= 65_536,
      let data = raw.data(using: .utf8),
      let incoming = try? JSONDecoder().decode(PageObservationMessage.self, from: data)
    else { return }
    switch incoming.kind {
    case "console": appendConsole(incoming)
    case "network": appendNetwork(incoming)
    default: return
    }
  }

  func consolePage(since: String?) throws -> JsonValue {
    let page = try buffer.consolePage(since: since)
    return .object([
      "entries": try encoded(page.values), "next": .string(String(page.next)),
      "truncated": .bool(page.truncated),
    ])
  }

  func networkPage(since: String?) throws -> JsonValue {
    let page = try buffer.networkPage(since: since)
    return .object([
      "entries": try encoded(page.values), "next": .string(String(page.next)),
      "truncated": .bool(page.truncated),
    ])
  }

  func consoleEntries() -> [WebConsoleEntry] {
    (try? buffer.consolePage(since: nil).values) ?? []
  }

  func networkEntries() -> [WebNetworkEntry] {
    (try? buffer.networkPage(since: nil).values) ?? []
  }

  func wait(args: JsonValue, timeoutMs: UInt32) async throws -> Bool {
    guard let webView else {
      throw ClientFailure.webHost(.internal_error, "The web pane is unavailable.")
    }
    let timeout = min(timeoutMs, UInt32(ProtocolLimits.webWaitTimeoutMs))
    let started = ContinuousClock.now
    repeat {
      if try await waitCondition(in: webView, args: args) { return true }
      if ContinuousClock.now - started >= .milliseconds(Int64(timeout)) { return false }
      try await Task.sleep(for: .milliseconds(50))
    } while !Task.isCancelled
    throw CancellationError()
  }

  private func waitCondition(in webView: WKWebView, args: JsonValue) async throws -> Bool {
    let raw = try await webView.callAsyncJavaScript(
      Self.waitScript,
      arguments: ["payload": try jsonObject(args)], in: nil, contentWorld: .page)
    guard let matched = raw as? Bool else {
      throw ClientFailure.webHost(.internal_error, "The page returned an invalid wait result.")
    }
    return matched
  }

  private func appendConsole(_ item: PageObservationMessage) {
    guard let level = item.level.flatMap(WebConsoleLevel.init(rawValue:)),
      let text = item.text.flatMap({ bounded($0, bytes: 16_384) }),
      let source = item.source.flatMap({ bounded($0, bytes: 4_096) }),
      let at = boundedTimestamp(item.at)
    else { return }
    guard (try? buffer.appendConsole(level: level, text: text, source: source, at: at)) != nil
    else { return }
    scheduleCountsReport()
  }

  private func appendNetwork(_ item: PageObservationMessage) {
    guard let method = item.method.flatMap({ bounded($0, bytes: 32) }), !method.isEmpty,
      let url = item.url.flatMap({ bounded($0, bytes: 4_096) }), !url.isEmpty,
      let status = item.status, status <= 999, let at = boundedTimestamp(item.at),
      let bytes = item.bytes.flatMap(UInt64.init)
    else { return }
    try? buffer.appendNetwork(
      method: method, url: url, status: status, durationMs: item.durationMs ?? 0, bytes: bytes,
      at: at)
  }

  private func scheduleCountsReport() {
    guard reportTask == nil else { return }
    reportTask = Task { [weak self] in
      do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
      guard let self else { return }
      self.reportTask = nil
      let counts = self.buffer.consoleCounts
      await self.onCountsChanged(counts.warnings, counts.errors)
    }
  }

  private func bounded(_ value: String, bytes limit: Int) -> String? {
    guard value.utf8.count <= limit else { return nil }
    return value
  }

  private func boundedTimestamp(_ value: String) -> String? {
    guard !value.isEmpty else { return nil }
    return bounded(value, bytes: 64)
  }

  private func encoded<T: Encodable>(_ value: T) throws -> JsonValue {
    try JSONDecoder().decode(JsonValue.self, from: JSONEncoder().encode(value))
  }

  private func jsonObject(_ value: JsonValue) throws -> Any {
    try JSONSerialization.jsonObject(with: JSONEncoder().encode(value))
  }
}

extension WebPageObservation {
  fileprivate static let waitScript = #"""
    const state = globalThis.__soyehtObservation || {active:0,lastActivity:0};
    const textMatches = !payload.text || String(document.body?.innerText || "").includes(payload.text);
    let selectorMatches = true;
    if (payload.selector) {
      try { selectorMatches = document.querySelector(payload.selector) !== null; }
      catch (_) { selectorMatches = false; }
    }
    const networkMatches = payload.networkIdle !== true ||
      (document.readyState === "complete" && state.active === 0 && Date.now() - state.lastActivity >= 500);
    return textMatches && selectorMatches && networkMatches;
    """#

  fileprivate static let observerScript = #"""
    (() => {
      const send = value => {
        try { webkit.messageHandlers[__HANDLER__].postMessage(JSON.stringify(value)); } catch (_) {}
      };
      const at = () => new Date().toISOString();
      const bounded = (value, limit) => new TextDecoder().decode(
        new TextEncoder().encode(String(value)).slice(0, limit));
      const text = value => {
        try {
          if (typeof value === "string") return bounded(value, 2048);
          const encoded = JSON.stringify(value);
          return bounded(encoded === undefined ? value : encoded, 2048);
        } catch (_) { return bounded(value, 2048); }
      };
      const source = () => bounded(location.href || "", 4096);
      const state = globalThis.__soyehtObservation = {active:0,lastActivity:Date.now()};
      for (const level of ["debug", "log", "info", "warn", "error"]) {
        const original = console[level].bind(console);
        console[level] = (...args) => {
          original(...args);
          send({kind:"console", level:level === "warn" ? "warning" : level === "log" ? "info" : level,
            text:bounded(args.slice(0,32).map(text).join(" "),16384), source:source(), at:at()});
        };
      }
      const finish = (started, method, url, status) => {
        state.active = Math.max(0, state.active - 1); state.lastActivity = Date.now();
        send({kind:"network", method:bounded(method,32), url:bounded(url,4096),
          status:Number(status) || 0, durationMs:Math.min(4294967295,Math.round(performance.now()-started)),
          bytes:"0", at:at()});
      };
      const originalFetch = globalThis.fetch.bind(globalThis);
      globalThis.fetch = (...args) => {
        const started = performance.now();
        const input = args[0], init = args[1] || {};
        const method = init.method || (input && input.method) || "GET";
        const url = (input && input.url) || input;
        state.active += 1; state.lastActivity = Date.now();
        return originalFetch(...args).then(response => {
          finish(started, method, response.url || url, response.status); return response;
        }, error => { finish(started, method, url, 0); throw error; });
      };
      const open = XMLHttpRequest.prototype.open;
      const sendRequest = XMLHttpRequest.prototype.send;
      XMLHttpRequest.prototype.open = function(method, url, ...rest) {
        this.__soyehtMethod = method; this.__soyehtURL = url; return open.call(this, method, url, ...rest);
      };
      XMLHttpRequest.prototype.send = function(...args) {
        const started = performance.now(); state.active += 1; state.lastActivity = Date.now();
        this.addEventListener("loadend", () => finish(started, this.__soyehtMethod || "GET",
          this.responseURL || this.__soyehtURL || "", this.status), {once:true});
        try { return sendRequest.apply(this, args); }
        catch (error) { finish(started, this.__soyehtMethod || "GET", this.__soyehtURL || "", 0); throw error; }
      };
    })();
    """#
}
