#if os(macOS)
  import AppKit
  import Foundation
  import SoyehtProtocol
  import WebKit

  @testable import SoyehtClient

  /// A web view with the wiring a Mac web pane gives it, loaded with a test page and never shown.
  @MainActor final class WebPageFixture: NSObject, WKNavigationDelegate {
    let webView: WKWebView
    let observation: WebPageObservation
    let actions: WebStructuredActions
    let reader: WebPageReader
    private var loaded: CheckedContinuation<Void, Never>?

    init(width: Double = 800, height: Double = 600) {
      _ = NSApplication.shared
      let configuration = WKWebViewConfiguration()
      configuration.applicationNameForUserAgent = SafariIdentity.applicationName
      webView = WKWebView(
        frame: CGRect(x: 0, y: 0, width: width, height: height), configuration: configuration)
      observation = WebPageObservation(onCountsChanged: { _, _ in })
      actions = WebStructuredActions(webView: webView)
      reader = WebPageReader(webView: webView)
      super.init()
      observation.bind(webView)
      webView.navigationDelegate = self
    }

    /// A fixture with `html` already loaded from a site that is not a development host.
    static func loaded(_ html: String = WebTestPages.tall()) async -> WebPageFixture {
      let fixture = WebPageFixture()
      await fixture.load(html, baseURL: "https://reader.example/")
      return fixture
    }

    /// Runs a read of `screens` screens whose `between` calls `action` on its `call`th check, and
    /// returns what the read threw (nil when it finished).
    func read(
      screens: Int = 6, failingAtCall call: Int, _ action: @escaping () throws -> Void
    ) async -> (any Error)? {
      var calls = 0
      do {
        _ = try await reader.read(
          screens: screens, collectsContent: false, deadline: .now + .seconds(60),
          between: {
            calls += 1
            if calls == call { try action() }
          })
        return nil
      } catch {
        return error
      }
    }

    func load(_ html: String, baseURL: String) async {
      await withCheckedContinuation { continuation in
        loaded = continuation
        webView.loadHTMLString(html, baseURL: URL(string: baseURL))
      }
    }

    func read(screens: Int, collectsContent: Bool = true) async throws -> WebReadCapture {
      try await reader.read(
        screens: screens, collectsContent: collectsContent, deadline: .now + .seconds(90),
        between: {})
    }

    func pageValue(_ script: String) async throws -> String {
      let raw = try await webView.callAsyncJavaScript(
        script, arguments: [:], in: nil, contentWorld: .page)
      return raw as? String ?? ""
    }

    func webView(
      _ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction
    ) async -> WKNavigationActionPolicy {
      if navigationAction.targetFrame?.isMainFrame == true {
        observation.navigationWillStart(to: navigationAction.request.url)
      }
      return .allow
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation?) { finish() }

    func webView(
      _ webView: WKWebView, didFail navigation: WKNavigation?, withError error: any Error
    ) { finish() }

    func webView(
      _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation?,
      withError error: any Error
    ) { finish() }

    private func finish() {
      loaded?.resume()
      loaded = nil
    }
  }

  enum WebTestPages {
    /// Ten 300 px blocks of known colours under a sticky bar, with a link in three of them.
    static func tall(blocks: Int = 10, barHeight: Int = 40, extraScript: String = "") -> String {
      var body =
        "<div style='position:sticky;top:0;height:\(barHeight)px;background:#222;color:#fff'>Sticky bar</div>"
      for index in 0..<blocks {
        let link =
          [0, 4, 9].contains(index)
          ? "<a href='/post/\(index)' style='display:block'>Open post \(index)</a>" : ""
        body +=
          "<div style='height:300px;background:hsl(\(WebTestPages.hue(index)),70%,60%)'><p style='margin:0'>Block \(index) text</p>\(link)</div>"
      }
      return page(body: body, script: extraScript)
    }

    /// A page whose document does not scroll: an app shell with one tall scroller inside it.
    static func innerScroller(rows: Int = 10) -> String {
      var body = "<div style='height:100vh;overflow:auto'>"
      for index in 0..<rows {
        body +=
          "<div style='height:300px;background:hsl(\(hue(index)),70%,60%)'><p style='margin:0'>Row \(index) text</p></div>"
      }
      return
        "<!doctype html><html style='overflow:hidden'><body style='margin:0;overflow:hidden;font:20px sans-serif'>\(body)</div></body></html>"
    }

    /// A page that adds four blocks whenever its sentinel nears the screen, as a timeline does.
    static let endless = page(
      body: "<div id=feed></div><div id=sentinel style='height:10px'></div>",
      script: """
        let n = 0;
        function more() { for (let i = 0; i < 4; i++) { const d = document.createElement('div');
          d.style.cssText = 'height:300px;background:hsl(' + WebTestPages.hue + ',70%,60%)';
          d.textContent = 'Post ' + n; n++; document.getElementById('feed').appendChild(d); } }
        more();
        new IntersectionObserver(e => { if (e[0].isIntersecting) more(); })
          .observe(document.getElementById('sentinel'));
        """.replacingOccurrences(of: "WebTestPages.hue", with: "(n * 37 % 360)"))

    static func hue(_ index: Int) -> Int { index * 37 % 360 }

    static func page(body: String, script: String) -> String {
      "<!doctype html><html><head><title>Test</title></head><body style='margin:0;font:20px sans-serif'>\(body)<script>\(script)</script></body></html>"
    }
  }
#endif
