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
        navigate: navigate, resize: resize, togglePicker: session.elementPicker.toggle)
      ZStack(alignment: .bottom) {
        viewportBody(session, viewport: session.viewport ?? pane.viewport)
        WebPickTray(
          client: client, pane: pane, picker: session.elementPicker,
          attach: attachSelection)
      }
      WebStatusBar(
        pane: pane, viewport: session.viewport ?? pane.viewport,
        view: client.webPanes[pane.paneId],
        servers: client.localServers)
    }
    .onChange(of: pane.url) { _, value in address = value ?? "" }
    .onDisappear { session.elementPicker.navigationStarted() }
  }

  @ViewBuilder private func viewportBody(_ session: MacWebSession, viewport: WebViewport?)
    -> some View
  {
    if let viewport, viewport.preset != .responsive {
      ScrollView([.horizontal, .vertical]) {
        HostedWebView(session: session, url: pane.url)
          .frame(width: CGFloat(viewport.width), height: CGFloat(viewport.height))
      }
      .background(MacTheme.face)
      .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous))
    } else {
      HostedWebView(session: session, url: pane.url)
        .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous))
    }
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

  private func resize(_ preset: WebViewportPreset, rotate: Bool) {
    Task {
      do {
        let bounds = host.session(for: pane).webView.bounds
        let width = preset == .responsive ? UInt32(max(1, bounds.width.rounded())) : nil
        let height = preset == .responsive ? UInt32(max(1, bounds.height.rounded())) : nil
        _ = try await client.resizeWeb(
          paneId: pane.paneId, preset: preset == .responsive ? nil : preset, width: width,
          height: height, rotate: rotate)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func attachSelection(to agentPaneId: String) {
    guard let selection = host.session(for: pane).elementPicker.takeSelection() else { return }
    client.attachWebElements(
      from: pane.paneId, package: selection.package, images: selection.images,
      to: agentPaneId)
  }
}

private struct WebNavigationBar: View {
  @Binding var address: String
  let pane: Pane
  let session: MacWebSession
  let navigate: (String?, WebNavigationAction?) -> Void
  let resize: (WebViewportPreset, Bool) -> Void
  let togglePicker: () -> Void

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
      viewportMenu
      pickButton
      navigationButton("arrow.up.right.square", "Open in system browser") { openExternally() }
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: 27)
    .neoWell(radius: MacTheme.Radius.pill)
  }

  private var pickButton: some View {
    Button(action: togglePicker) {
      HStack(spacing: MacTheme.Space.x6) {
        Image(systemName: "cursorarrow.click.2").frame(width: 11, height: 11)
        Text("Pick element").font(MacTheme.chrome(MacTheme.TypeScale.micro))
      }
      .foregroundStyle(session.elementPicker.isActive ? MacTheme.accent : MacTheme.ink)
      .padding(.horizontal, MacTheme.Space.x8).frame(height: 20)
      .background(
        session.elementPicker.isActive ? MacTheme.well : MacTheme.face,
        in: Capsule())
    }
    .buttonStyle(.plain).help("Pick an element for an agent request")
  }

  private var viewportMenu: some View {
    Menu {
      viewportButton("Responsive", .responsive)
      viewportButton("iPhone SE", .iphone_se)
      viewportButton("iPhone 15 Pro", .iphone15_pro)
      viewportButton("iPad", .ipad)
      Divider()
      Button("Rotate") { resize(session.viewport?.preset ?? .responsive, true) }
    } label: {
      Image(systemName: "rectangle.on.rectangle")
        .frame(width: 12, height: 12)
        .foregroundStyle(MacTheme.ink)
    }
    .menuStyle(.borderlessButton)
    .menuIndicator(.hidden)
    .fixedSize()
    .help("Viewport")
    .accessibilityLabel("Viewport")
  }

  private func viewportButton(_ label: String, _ preset: WebViewportPreset) -> some View {
    Button(label) { resize(preset, false) }
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

private struct WebPickTray: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let picker: WebElementPicker
  let attach: (String) -> Void

  private var agents: [Pane] {
    client.panes.filter { $0.workspaceId == pane.workspaceId && $0.mode == .agent }
  }

  private var preferred: Pane? {
    client.preferredAgentPane(in: pane.workspaceId) ?? agents.first
  }

  var body: some View {
    if picker.isActive || picker.selection != nil {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: "cursorarrow.click.2").foregroundStyle(MacTheme.accent)
        Text(message).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        Spacer()
        if picker.selection != nil, let preferred {
          Button {
            attach(preferred.paneId)
          } label: {
            Label("Attach to \(preferred.name)", systemImage: "paperclip")
              .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          }.buttonStyle(.plain)
          Menu {
            ForEach(agents, id: \.paneId) { agent in
              Button(agent.name) { attach(agent.paneId) }
            }
          } label: {
            Image(systemName: "chevron.down")
          }.menuStyle(.borderlessButton).fixedSize()
        }
        Button("Clear") { picker.navigationStarted() }
          .font(MacTheme.chrome(MacTheme.TypeScale.small)).buttonStyle(.plain)
      }
      .padding(.horizontal, MacTheme.Space.x12).frame(height: 34)
      .neoRaised(radius: MacTheme.Radius.pill, elevation: .card)
      .padding(MacTheme.Space.x12)
    }
  }

  private var message: String {
    if !agents.isEmpty, !picker.elements.isEmpty {
      return "\(picker.elements.count) selected · Shift-click to add"
    }
    if agents.isEmpty { return "Open an agent pane to attach this selection" }
    return "Click an element · Shift-click to select up to 20"
  }
}

private struct WebStatusBar: View {
  let pane: Pane
  let viewport: WebViewport?
  let view: WebPaneView?
  let servers: [LocalServer]

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      if let server {
        Text("Local server · port \(server.port) · pid \(server.pid)")
          .font(MacTheme.mono(MacTheme.TypeScale.micro))
      }
      if let viewport {
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
