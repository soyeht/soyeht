import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UniformTypeIdentifiers
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
        navigate: navigate, resize: resize, togglePicker: session.elementPicker.toggle,
        reportProblem: { client.problem = $0 })
      ZStack(alignment: .bottom) {
        viewportBody(session, viewport: session.viewport ?? pane.viewport)
        WebPickTray(
          client: client, pane: pane, picker: session.elementPicker,
          attach: attachSelection)
      }
      .overlay(alignment: .topTrailing) {
        WebControlBadge(
          session: session, view: client.webPanes[pane.paneId], url: pane.url)
      }
      WebStatusBar(
        pane: pane, viewport: session.viewport ?? pane.viewport,
        view: client.webPanes[pane.paneId], session: session,
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
    host.session(for: pane).noteHumanInput()
    Task {
      do {
        _ = try await client.navigateWeb(paneId: pane.paneId, url: url, action: action)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func resize(_ preset: WebViewportPreset, rotate: Bool) {
    host.session(for: pane).noteHumanInput()
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

private struct WebControlBadge: View {
  let session: MacWebSession
  let view: WebPaneView?
  let url: String?

  var body: some View {
    if session.humanHasControl {
      Button(action: session.handBack) {
        Label("Your turn · Hand control back", systemImage: "hand.raised.fill")
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.ink)
          .padding(.horizontal, MacTheme.Space.x10)
          .frame(height: 28)
          .neoRaised(radius: MacTheme.Radius.pill, elevation: .card)
      }
      .buttonStyle(.plain)
      .padding(MacTheme.Space.x12)
    } else if session.agentCommandRunning || view?.agentDriving == true {
      Label("Agent driving · \(domain)", systemImage: "cursorarrow.motionlines")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.accent)
        .padding(.horizontal, MacTheme.Space.x10)
        .frame(height: 28)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .card)
        .padding(MacTheme.Space.x12)
    }
  }

  private var domain: String {
    view?.drivingDomain ?? URL(string: url ?? "")?.host ?? "web pane"
  }
}

private struct WebNavigationBar: View {
  @Binding var address: String
  let pane: Pane
  let session: MacWebSession
  let navigate: (String?, WebNavigationAction?) -> Void
  let resize: (WebViewportPreset, Bool) -> Void
  let togglePicker: () -> Void
  let reportProblem: (String) -> Void
  @State private var findPresented = false
  @State private var inspectorPresented = false
  @State private var findText = ""
  @State private var findMatched: Bool?

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
      zoomMenu
      findButton
      pickButton
      inspectorButton
      screenshotMenu
      navigationButton("arrow.up.right.square", "Open in system browser") { openExternally() }
    }
    .padding(.horizontal, MacTheme.Space.x10)
    .frame(height: 27)
    .neoWell(radius: MacTheme.Radius.pill)
  }

  private var zoomMenu: some View {
    Menu {
      Button("Zoom out") { setZoom(session.zoomPercent.saturatingSubtract(10)) }
      Button("Actual size") { setZoom(100) }
      Button("Zoom in") { setZoom(min(200, session.zoomPercent + 10)) }
    } label: {
      Text("\(session.zoomPercent)%")
        .font(MacTheme.mono(MacTheme.TypeScale.micro))
        .foregroundStyle(MacTheme.ink)
    }
    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Zoom")
  }

  private var findButton: some View {
    navigationButton("magnifyingglass", "Find in page") { findPresented.toggle() }
      .popover(isPresented: $findPresented) {
        VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
          Text("Find in page")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          TextField("Text", text: $findText)
            .font(MacTheme.chrome(MacTheme.TypeScale.caption))
            .textFieldStyle(.plain).padding(.horizontal, MacTheme.Space.x8)
            .frame(width: 220, height: 26).neoWell(radius: MacTheme.Radius.compact)
            .onSubmit { find(backwards: false) }
          HStack {
            Button("Previous") { find(backwards: true) }
            Button("Next") { find(backwards: false) }
            Spacer()
            if let findMatched {
              Text(findMatched ? "Match found" : "No match")
                .font(MacTheme.chrome(MacTheme.TypeScale.micro))
                .foregroundStyle(findMatched ? MacTheme.success : MacTheme.muted)
            }
          }.buttonStyle(.plain)
        }.padding(MacTheme.Space.x12)
      }
  }

  private var inspectorButton: some View {
    navigationButton("wrench.and.screwdriver", "Web Inspector") {
      session.webView.isInspectable = true
      inspectorPresented.toggle()
    }
    .popover(isPresented: $inspectorPresented) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
        Text("Web Inspector")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        Text("This pane is inspectable. Choose it from Safari’s Develop menu.")
          .font(MacTheme.chrome(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
          .frame(width: 240, alignment: .leading)
        Button("Open Safari") { openSafari() }
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .buttonStyle(.plain).foregroundStyle(MacTheme.accent)
      }.padding(MacTheme.Space.x12)
    }
  }

  private var screenshotMenu: some View {
    Menu {
      Button("Copy viewport") { screenshot(fullPage: false, destination: .clipboard) }
      Button("Copy full page") { screenshot(fullPage: true, destination: .clipboard) }
      Divider()
      Button("Save viewport…") { screenshot(fullPage: false, destination: .file) }
      Button("Save full page…") { screenshot(fullPage: true, destination: .file) }
    } label: {
      Image(systemName: "camera").frame(width: 12, height: 12).foregroundStyle(MacTheme.ink)
    }
    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
    .help("Screenshot").accessibilityLabel("Screenshot")
  }

  private func find(backwards: Bool) {
    session.noteHumanInput()
    Task { findMatched = await session.find(findText, backwards: backwards) }
  }

  private func screenshot(fullPage: Bool, destination: ScreenshotDestination) {
    session.noteHumanInput()
    Task {
      do {
        let data = try await session.humanScreenshot(fullPage: fullPage)
        switch destination {
        case .clipboard:
          NSPasteboard.general.clearContents()
          guard NSPasteboard.general.setData(data, forType: .png) else {
            throw CocoaError(.fileWriteUnknown)
          }
        case .file:
          try saveScreenshot(data)
        }
      } catch { reportProblem(error.localizedDescription) }
    }
  }

  private func saveScreenshot(_ data: Data) throws {
    let panel = NSSavePanel()
    panel.allowedContentTypes = [.png]
    panel.nameFieldStringValue = "screenshot.png"
    guard panel.runModal() == .OK, let url = panel.url else { return }
    try data.write(to: url, options: .atomic)
  }

  private func openSafari() {
    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Safari")
    else {
      reportProblem("Safari is unavailable.")
      return
    }
    Task {
      do {
        _ = try await NSWorkspace.shared.openApplication(at: url, configuration: .init())
      } catch {
        reportProblem(error.localizedDescription)
      }
    }
  }

  private var pickButton: some View {
    Button {
      session.noteHumanInput()
      togglePicker()
    } label: {
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
        .simultaneousGesture(TapGesture().onEnded { session.noteHumanInput() })
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
    Button {
      session.noteHumanInput()
      action()
    } label: {
      Image(systemName: icon)
        .frame(width: 12, height: 12)
        .foregroundStyle(enabled ? MacTheme.ink : MacTheme.muted)
    }
    .buttonStyle(.plain)
    .disabled(!enabled)
    .help(label)
    .accessibilityLabel(label)
  }

  private func setZoom(_ percent: UInt32) {
    session.noteHumanInput()
    session.setZoomPercent(percent)
  }
}

private enum ScreenshotDestination { case clipboard, file }

extension UInt32 {
  fileprivate func saturatingSubtract(_ value: UInt32) -> UInt32 { self > value ? self - value : 0 }
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
  let session: MacWebSession
  let servers: [LocalServer]
  @State private var observationsPresented = false

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
      Button {
        observationsPresented.toggle()
      } label: {
        HStack(spacing: MacTheme.Space.x6) {
          Image(systemName: "terminal").foregroundStyle(MacTheme.warning)
          Text("console: \(view?.warnings ?? 0) warnings · \(view?.errors ?? 0) errors")
            .font(MacTheme.chrome(MacTheme.TypeScale.micro))
        }
      }
      .buttonStyle(.plain)
      .help("Console and network activity")
      .popover(isPresented: $observationsPresented) {
        WebObservationPanel(session: session)
      }
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

private enum WebObservationSection: String, CaseIterable, Identifiable {
  case console = "Console"
  case network = "Network"
  var id: Self { self }
}

private struct WebObservationPanel: View {
  let session: MacWebSession
  @State private var section = WebObservationSection.console

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      Picker("Activity", selection: $section) {
        ForEach(WebObservationSection.allCases) { Text($0.rawValue).tag($0) }
      }
      .pickerStyle(.segmented)
      .labelsHidden()
      TimelineView(.periodic(from: .now, by: 1)) { _ in
        ScrollView {
          LazyVStack(alignment: .leading, spacing: MacTheme.Space.x8) {
            if section == .console { consoleRows } else { networkRows }
          }
        }
      }
    }
    .padding(MacTheme.Space.x12)
    .frame(width: 480, height: 320)
    .background(MacTheme.canvas)
  }

  @ViewBuilder private var consoleRows: some View {
    let entries = session.consoleEntries().reversed()
    if entries.isEmpty {
      empty("No console messages")
    } else {
      ForEach(Array(entries), id: \.sequence) { entry in
        VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
          HStack {
            Text(entry.level.rawValue.uppercased())
              .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
              .foregroundStyle(consoleColor(entry.level))
            Spacer()
            Text(entry.at).font(MacTheme.mono(MacTheme.TypeScale.micro))
          }
          Text(entry.text)
            .font(MacTheme.mono(MacTheme.TypeScale.small))
            .foregroundStyle(MacTheme.ink)
            .textSelection(.enabled)
          Text(entry.source)
            .font(MacTheme.mono(MacTheme.TypeScale.micro))
            .foregroundStyle(MacTheme.muted)
            .lineLimit(1)
        }
        .padding(MacTheme.Space.x8)
        .neoWell(radius: MacTheme.Radius.compact)
      }
    }
  }

  @ViewBuilder private var networkRows: some View {
    let entries = session.networkEntries().reversed()
    if entries.isEmpty {
      empty("No network requests")
    } else {
      ForEach(Array(entries), id: \.sequence) { entry in
        VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
          HStack(spacing: MacTheme.Space.x8) {
            Text(entry.method).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
            Text(String(entry.status)).font(MacTheme.mono(MacTheme.TypeScale.micro))
            Text("\(entry.durationMs) ms").font(MacTheme.mono(MacTheme.TypeScale.micro))
            Spacer()
            Text("\(entry.bytes) bytes").font(MacTheme.mono(MacTheme.TypeScale.micro))
          }
          Text(entry.url)
            .font(MacTheme.mono(MacTheme.TypeScale.small))
            .foregroundStyle(MacTheme.ink)
            .lineLimit(2)
            .textSelection(.enabled)
        }
        .padding(MacTheme.Space.x8)
        .neoWell(radius: MacTheme.Radius.compact)
      }
    }
  }

  private func empty(_ title: String) -> some View {
    Text(title)
      .font(MacTheme.chrome(MacTheme.TypeScale.small))
      .foregroundStyle(MacTheme.muted)
      .frame(maxWidth: .infinity, minHeight: 220)
  }

  private func consoleColor(_ level: WebConsoleLevel) -> Color {
    switch level {
    case .debug, .info: MacTheme.secondary
    case .warning: MacTheme.warning
    case .error: MacTheme.danger
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
