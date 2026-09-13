import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI
import WebKit

struct WebPaneSurface: View {
  @Bindable var client: EngineClient
  let host: MacWebHost
  let pane: Pane
  @State private var address: String

  init(client: EngineClient, host: MacWebHost, pane: Pane) {
    self.client = client
    self.host = host
    self.pane = pane
    _address = State(initialValue: pane.url ?? "")
  }

  var body: some View {
    let session = host.session(for: pane)
    VStack(spacing: MacTheme.Space.x8) {
      WebNavigationBar(
        address: $address, pane: pane, session: session,
        navigate: navigate)
      HostedWebView(session: session, url: pane.url)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous))
      WebStatusBar(pane: pane, view: client.webPanes[pane.paneId], servers: client.localServers)
    }
    .onChange(of: pane.url) { _, value in address = value ?? "" }
  }

  private func navigate(url: String?, action: WebNavigationAction?) {
    Task {
      do {
        _ = try await client.navigateWeb(paneId: pane.paneId, url: url, action: action)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct WebNavigationBar: View {
  @Binding var address: String
  let pane: Pane
  let session: MacWebSession
  let navigate: (String?, WebNavigationAction?) -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      navigationButton("chevron.left", "Back", enabled: session.canGoBack) {
        navigate(nil, .back)
      }
      navigationButton("chevron.right", "Forward", enabled: session.canGoForward) {
        navigate(nil, .forward)
      }
      navigationButton("arrow.clockwise", "Reload") {
        navigate(nil, NSEvent.modifierFlags.contains(.shift) ? .reload_ignoring_cache : .reload)
      }
      addressField
      navigationButton("arrow.up.right.square", "Open in system browser") { openExternally() }
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: 27)
    .neoWell(radius: MacTheme.Radius.pill)
  }

  private var addressField: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: secure ? "lock.fill" : "exclamationmark.triangle.fill")
        .foregroundStyle(secure ? MacTheme.muted : MacTheme.warning)
      TextField("Address", text: $address)
        .textFieldStyle(.plain)
        .font(MacTheme.mono(MacTheme.TypeScale.small))
        .onSubmit { navigate(address, nil) }
      if let label = serverLabel {
        Text(label)
          .font(MacTheme.chrome(MacTheme.TypeScale.micro))
          .foregroundStyle(MacTheme.muted)
          .lineLimit(1)
      }
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: 20)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
  }

  private var secure: Bool { URL(string: pane.url ?? "")?.scheme == "https" }

  private var serverLabel: String? {
    guard let host = URL(string: pane.url ?? "")?.host,
      ["localhost", "127.0.0.1", "::1"].contains(host)
    else { return nil }
    return pane.title?.isEmpty == false ? pane.title : "Local server"
  }

  private func openExternally() {
    guard let raw = pane.url, let url = URL(string: raw), ["http", "https"].contains(url.scheme)
    else { return }
    NSWorkspace.shared.open(url)
  }

  private func navigationButton(
    _ icon: String, _ label: String, enabled: Bool = true, action: @escaping () -> Void
  ) -> some View {
    Button(action: action) {
      Image(systemName: icon)
        .frame(width: 12, height: 12)
        .foregroundStyle(enabled ? MacTheme.ink : MacTheme.muted)
    }
    .buttonStyle(.plain)
    .disabled(!enabled)
    .help(label)
    .accessibilityLabel(label)
  }
}

private struct WebStatusBar: View {
  let pane: Pane
  let view: WebPaneView?
  let servers: [LocalServer]

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      if let server {
        Text("Local server · port \(server.port) · pid \(server.pid)")
          .font(MacTheme.mono(MacTheme.TypeScale.micro))
      }
      if let viewport = pane.viewport {
        Text("\(viewport.width) × \(viewport.height)")
          .font(MacTheme.mono(MacTheme.TypeScale.micro))
      }
      Spacer(minLength: 0)
      Image(systemName: "terminal").foregroundStyle(MacTheme.warning)
      Text("console: \(view?.warnings ?? 0) warnings · \(view?.errors ?? 0) errors")
        .font(MacTheme.chrome(MacTheme.TypeScale.micro))
      Text(sessionLabel)
        .font(MacTheme.chrome(MacTheme.TypeScale.micro))
    }
    .foregroundStyle(MacTheme.muted)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: 24)
    .background(MacTheme.canvas)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Space.x8, style: .continuous))
  }

  private var server: LocalServer? {
    guard let url = URL(string: pane.url ?? ""), let port = url.port else { return nil }
    return servers.first { $0.port == port }
  }

  private var sessionLabel: String {
    switch pane.webSession {
    case .anonymous, nil: "session: signed out"
    case .permission_required: "session: permission required"
    case .granted: "session: this Mac profile"
    case .incognito: "session: incognito"
    }
  }
}

private struct HostedWebView: NSViewRepresentable {
  let session: MacWebSession
  let url: String?

  func makeNSView(context: Context) -> WKWebView {
    session.webView
  }

  func updateNSView(_ webView: WKWebView, context: Context) {
    session.loadIfNeeded(url)
  }
}
