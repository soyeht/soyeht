import SoyehtProtocol
import SwiftUI
import WebKit

struct WebPaneSurface: NSViewRepresentable {
  let host: MacWebHost
  let pane: Pane

  func makeNSView(context: Context) -> WKWebView {
    host.session(for: pane).webView
  }

  func updateNSView(_ webView: WKWebView, context: Context) {
    host.session(for: pane).loadIfNeeded(pane.url)
  }
}
