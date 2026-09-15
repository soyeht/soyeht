import AppKit
import CoreImage.CIFilterBuiltins
import SoyehtClient
import SoyehtProtocol
import SwiftUI

enum PaneSplitAxis: Sendable {
  case columns
  case rows
}

struct WorkspaceWindow: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  @State private var navigation = WorkspaceNavigation()
  @State private var editorInputPreferences = EditorInputPreferences()
  @State private var webLinkPreferences = WebLinkPreferences()
  @State private var paneToClose: String?
  @State private var showNewPane = false
  @State private var paneSplitAxis = PaneSplitAxis.columns
  @State private var showPairing = false
  @State private var deviceToRevoke: String?
  @State private var taskWorkspaceId: String?
  @State private var showPreferences = false
  @State private var selectedUsageProvider: UsageProvider?
  @State private var editorPaletteMode: EditorPaletteMode?
  var body: some View {
    VStack(spacing: MacTheme.Space.x16) {
      WorkspaceTopBar(
        client: client, workspaceId: selectedTaskWorkspaceId,
        onSelectWorkspace: selectWorkspace, onNewPane: presentNewPane,
        onTasks: showTasks, onToggleAppBar: { navigation.toggleAppBar() })
      HStack(spacing: 0) {
        if navigation.appBarOpen {
          AppRail(
            client: client,
            destination: railDestination,
            onSelect: selectDestination,
            onSettings: {
              selectedUsageProvider = nil
              showPreferences = true
            },
            onUsageDetails: { provider in
              selectedUsageProvider = provider
              showPreferences = true
            }
          )
          .transition(.move(edge: .leading).combined(with: .opacity))
          .zIndex(30)
        }
        workspaceContent
      }
    }
    .padding(EdgeInsets(top: 14, leading: 18, bottom: 18, trailing: 18))
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink).buttonStyle(.plain)
    .environment(editorInputPreferences)
    .environment(webLinkPreferences)
    .sheet(isPresented: $showPairing, onDismiss: { client.clearPairCode() }) { pairing }
    .alert(
      "Close pane?",
      isPresented: Binding(get: { paneToClose != nil }, set: { if !$0 { paneToClose = nil } })
    ) {
      Button("Cancel", role: .cancel) { paneToClose = nil }
      Button("Close", role: .destructive) {
        if let id = paneToClose { client.closePane(id) }
        paneToClose = nil
      }
    } message: {
      Text("The process in this pane will be terminated.")
    }
    .alert(
      "Could not complete",
      isPresented: Binding(
        get: { client.problem != nil }, set: { if !$0 { client.problem = nil } })
    ) {
      Button("OK") { client.problem = nil }
    } message: {
      Text(client.problem ?? "")
    }
    .overlay(alignment: .bottom) {
      if let request = client.pairRequests.first {
        VStack(alignment: .leading, spacing: 12) {
          Text("Allow access for \(request.deviceName)?")
            .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          Text("Compare this fingerprint on the iPhone:")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          Text(request.fingerprint).font(MacTheme.mono(MacTheme.TypeScale.control))
            .textSelection(.enabled)
          HStack {
            Button("Deny") { client.decidePairing(request, approve: false) }
            Spacer()
            Button("Allow") { client.decidePairing(request, approve: true) }.buttonStyle(
              .borderedProminent)
          }
        }.padding(22).frame(width: 420).neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
          .padding(.bottom, 40)
      }
    }
    .overlay {
      if showPreferences {
        ZStack {
          Color.clear.contentShape(Rectangle()).onTapGesture { showPreferences = false }
          PreferencesPanel(client: client, initialUsageProvider: selectedUsageProvider)
            .onTapGesture {}
            .transition(.scale(scale: 0.98).combined(with: .opacity))
        }
        .zIndex(40)
      }
    }
    .overlay {
      if editorPaletteMode != nil {
        ZStack {
          MacTheme.ink.opacity(0.12).contentShape(Rectangle())
            .onTapGesture { editorPaletteMode = nil }
          EditorNavigationPalette(
            client: client, workspaceId: selectedTaskWorkspaceId, mode: $editorPaletteMode
          )
          .onTapGesture {}
        }
        .zIndex(60)
      }
    }
    .background(
      EditorDoubleShiftMonitor {
        editorPaletteMode = .everywhere
      }
    )
    .environment(
      \.filePathOpenAction,
      FilePathOpenAction { workspaceId, reference in
        openFileReference(workspaceId: workspaceId, reference: reference)
      }
    )
    .environment(
      \.webLinkOpenAction,
      WebLinkOpenAction { workspaceId, url in
        openWebLink(workspaceId: workspaceId, url: url)
      }
    )
    .onReceive(NotificationCenter.default.publisher(for: .showSoyehtPreferences)) { _ in
      selectedUsageProvider = nil
      showPreferences = true
    }
    .onReceive(NotificationCenter.default.publisher(for: .toggleSoyehtAppBar)) { _ in
      navigation.toggleAppBar()
    }
    .onReceive(NotificationCenter.default.publisher(for: .selectSoyehtWorkspaceView)) { note in
      if let destination = note.object as? WorkspaceDestination {
        selectDestination(destination)
      }
    }
    .onReceive(NotificationCenter.default.publisher(for: .showSoyehtEditorPalette)) { note in
      editorPaletteMode = note.object as? EditorPaletteMode ?? .commands
    }
    .onAppear { webHost.reconcile(client.panes) }
    .onChange(of: client.panes.map(\.paneId)) { _, _ in webHost.reconcile(client.panes) }
    .onChange(of: client.connectionRevision) { _, _ in webHost.resynchronize() }
    .onExitCommand { showPreferences = false }
  }

  @ViewBuilder private var workspaceContent: some View {
    switch navigation.destination {
    case .tasks:
      if let workspaceId = selectedTaskWorkspaceId {
        TaskWorkspaceView(client: client, workspaceId: workspaceId, onOpenPane: openPane)
      } else {
        EmptyWorkspaceView(icon: "checklist", title: "No workspace selected")
      }
    case .agents:
      if showNewPane {
        NewPaneWorkspace(
          client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId,
          splitAxis: paneSplitAxis, isPresented: $showNewPane,
          onClose: { paneToClose = $0 }, onSplit: presentSplit, onTasks: showTasks)
      } else {
        PaneGrid(
          client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: nil,
          splitAxis: paneSplitAxis, onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
          onSplit: presentSplit, onTasks: showTasks)
      }
    case .terminal:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .terminal,
        splitAxis: paneSplitAxis, onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .editor:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .editor,
        splitAxis: paneSplitAxis, onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .web:
      WebWorkspace(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId,
        onClose: { paneToClose = $0 }, onNewPane: presentNewPane, onSplit: presentSplit,
        onTasks: showTasks)
    case .phone:
      phoneWorkspace
    case .texts, .presentations, .screenplays:
      if let workspaceId = selectedTaskWorkspaceId,
        let section = navigation.destination.writingSection
      {
        WritingSectionView(
          client: client, workspaceId: workspaceId, section: section,
          onOpen: { paneId in
            client.select(paneId)
            navigation.select(.agents)
          })
      } else {
        EmptyWorkspaceView(icon: "text.book.closed", title: "No workspace selected")
      }
    }
  }

  private func selectWorkspace(_ workspaceId: String) {
    taskWorkspaceId = workspaceId
    if let pane = client.panes.first(where: { $0.workspaceId == workspaceId }) {
      client.select(pane.paneId)
    }
  }
  private func showTasks() {
    taskWorkspaceId = selectedTaskWorkspaceId
    selectDestination(.tasks)
  }
  private func beginPairing() {
    client.beginPairing()
    showPairing = true
  }
  private func presentNewPane() {
    paneSplitAxis = .columns
    client.refreshHarnesses()
    navigation.select(.agents)
    showNewPane = true
  }
  private func presentSplit(_ paneId: String, _ axis: PaneSplitAxis) {
    client.select(paneId)
    paneSplitAxis = axis
    client.refreshHarnesses()
    navigation.select(.agents)
    showNewPane = true
  }
  private var selectedTaskWorkspaceId: String? {
    taskWorkspaceId ?? client.selectedPane?.workspaceId ?? client.workspaces.first?.workspaceId
  }
  private var railDestination: WorkspaceDestination {
    guard navigation.destination == .agents, client.selectedPane?.mode == .writing,
      let section = client.selectedPane?.writingSection
    else { return navigation.destination }
    return section.destination
  }
  private func openPane(_ paneId: String) {
    client.select(paneId)
    navigation.select(.agents)
  }
  private func openFileReference(workspaceId: String, reference: FilePathReference) {
    Task { @MainActor in
      do {
        _ = try await client.openEditorFile(
          workspaceId: workspaceId, path: reference.path, line: reference.line,
          column: reference.column)
        taskWorkspaceId = workspaceId
        navigation.select(.editor)
      } catch { client.problem = error.localizedDescription }
    }
  }
  private func openWebLink(workspaceId: String, url: URL) {
    switch webLinkPreferences.destination(for: url.absoluteString) {
    case .soyeht(let target):
      Task { @MainActor in
        do {
          _ = try await client.openWeb(workspaceId: workspaceId, url: target.absoluteString)
          taskWorkspaceId = workspaceId
          navigation.select(.web)
        } catch { client.problem = error.localizedDescription }
      }
    case .system(let target): NSWorkspace.shared.open(target)
    case nil: client.problem = "Only HTTP and HTTPS links can be opened."
    }
  }
  private func selectDestination(_ destination: WorkspaceDestination) {
    navigation.select(destination)
    if destination != .agents { showNewPane = false }
  }
  private var pairing: some View {
    ScrollView { pairingContent }.frame(width: 520, height: 720).background(MacTheme.canvas)
  }
  private var pairingContent: some View {
    VStack(spacing: 20) {
      Text("Continue on iPhone").font(MacTheme.chrome(24, weight: .semibold))
      Text(
        "Open Soyeht Dev on iPhone and scan this code.\nBoth devices must be on the same tailnet."
      )
      .multilineTextAlignment(.center).foregroundStyle(MacTheme.secondary)
      if let code = client.pairCode, let image = qr(code.uri) {
        Image(nsImage: image).interpolation(.none).resizable().frame(width: 260, height: 260)
          .padding(MacTheme.Space.x16)
          .background(
            MacTheme.onAccent, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
        Text("Code valid for 10 minutes.").font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      } else {
        ProgressView().frame(width: 292, height: 292)
      }
      if let request = client.pairRequests.first {
        Text(request.deviceName)
          .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
        Text(request.fingerprint).font(MacTheme.mono(MacTheme.TypeScale.small))
          .textSelection(.enabled)
        HStack {
          Button("Deny") { client.decidePairing(request, approve: false) }
          Button("Allow") {
            client.decidePairing(request, approve: true)
            showPairing = false
          }
          .buttonStyle(.borderedProminent)
        }
      }
      if !client.devices.isEmpty {
        VStack(alignment: .leading, spacing: 12) {
          Text("Paired iPhones")
            .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          ForEach(client.devices, id: \.deviceId) { device in
            HStack {
              Label(device.name, systemImage: "iphone")
              Spacer()
              Button("Revoke", role: .destructive) { deviceToRevoke = device.deviceId }
            }
          }
        }.padding(.vertical, 12)
      }
      Button("Close") { showPairing = false }
    }.padding(32).frame(width: 460).background(MacTheme.canvas).foregroundStyle(MacTheme.ink)
      .confirmationDialog(
        "Revoke this iPhone's access?",
        isPresented: Binding(
          get: { deviceToRevoke != nil }, set: { if !$0 { deviceToRevoke = nil } })
      ) {
        Button("Revoke", role: .destructive) {
          if let id = deviceToRevoke { client.revokeDevice(id) }
          deviceToRevoke = nil
        }
        Button("Cancel", role: .cancel) { deviceToRevoke = nil }
      } message: {
        Text("Connections will close. Access will require pairing again.")
      }
  }

  private var phoneWorkspace: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x18) {
      HStack {
        VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
          Text("iPhone")
            .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
          Text("Paired devices can continue every workspace.")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption))
            .foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Button("Pair iPhone", action: beginPairing)
          .buttonStyle(NeoPillButtonStyle(accent: true))
      }
      if client.devices.isEmpty {
        EmptyWorkspaceView(icon: "iphone", title: "No paired iPhone")
      } else {
        VStack(spacing: MacTheme.Space.x10) {
          ForEach(client.devices, id: \.deviceId) { device in
            HStack(spacing: MacTheme.Space.x12) {
              Image(systemName: "iphone")
                .font(.system(size: 18, weight: .medium))
                .foregroundStyle(MacTheme.accent)
              VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
                Text(device.name)
                  .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
                Text(device.fingerprint)
                  .font(MacTheme.mono(MacTheme.TypeScale.small))
                  .foregroundStyle(MacTheme.secondary)
              }
              Spacer()
              Text(device.revoked ? "Revoked" : "Paired")
                .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
                .foregroundStyle(device.revoked ? MacTheme.danger : MacTheme.success)
            }
            .padding(MacTheme.Space.x14)
            .neoRaised(radius: MacTheme.Radius.control, elevation: .chip)
          }
        }
      }
    }
    .padding(EdgeInsets(top: 8, leading: 14, bottom: 6, trailing: 6))
  }
  private func qr(_ value: String) -> NSImage? {
    let filter = CIFilter.qrCodeGenerator()
    filter.message = Data(value.utf8)
    guard let image = filter.outputImage,
      let cg = CIContext().createCGImage(image, from: image.extent)
    else { return nil }
    return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
  }
}

private struct WorkspaceTopBar: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  let onSelectWorkspace: (String) -> Void
  let onNewPane: () -> Void
  let onTasks: () -> Void
  let onToggleAppBar: () -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Color.clear.frame(width: 52, height: 34)
      Button(action: onToggleAppBar) {
        Image(systemName: "rectangle.grid.2x2")
          .font(.system(size: 15, weight: .semibold)).frame(width: 34, height: 34)
          .neoRaised(radius: MacTheme.Radius.control)
      }
      .help("Toggle app bar")
      .accessibilityIdentifier("toggle-app-bar")

      ScrollView(.horizontal, showsIndicators: false) {
        HStack(spacing: MacTheme.Space.x10) {
          ForEach(client.workspaces, id: \.workspaceId) { workspace in
            WorkspaceTab(
              name: workspace.name, selected: workspace.workspaceId == workspaceId,
              action: { onSelectWorkspace(workspace.workspaceId) })
          }
        }.padding(.horizontal, 5).padding(.vertical, 5)
      }
      .clipped()
      .overlay(alignment: .trailing) {
        MacTheme.canvas.frame(width: MacTheme.Space.x18).allowsHitTesting(false)
      }

      Button(action: onNewPane) {
        Image(systemName: "plus").font(.system(size: 14, weight: .semibold))
          .frame(width: 34, height: 34).neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
      }
      .help("Open a terminal or agent pane").disabled(!client.connected)
      Spacer(minLength: MacTheme.Space.x10)
      HStack(spacing: MacTheme.Space.x6) {
        Circle().fill(client.connected ? MacTheme.success : MacTheme.warning).frame(
          width: 7, height: 7)
        Text(client.connected ? "Connected" : "Reconnecting")
          .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .semibold))
          .foregroundStyle(MacTheme.secondary)
      }
      Button(action: onTasks) {
        Label("Tasks", systemImage: "cpu")
      }.buttonStyle(NeoPillButtonStyle())
    }.frame(height: 42)
  }
}

private struct WorkspaceTab: View {
  let name: String
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack(spacing: MacTheme.Space.x8) {
        Image(systemName: "folder").font(.system(size: 14, weight: .medium))
          .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
        Text(name).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .foregroundStyle(selected ? MacTheme.accent : MacTheme.ink).lineLimit(1)
        Image(systemName: "xmark").font(.system(size: 9, weight: .semibold))
          .foregroundStyle(MacTheme.muted).accessibilityHidden(true)
      }
      .padding(.horizontal, MacTheme.Space.x12).frame(height: 36)
      .modifier(WorkspaceTabSurface(selected: selected))
    }
  }
}

private struct WorkspaceTabSurface: ViewModifier {
  let selected: Bool

  func body(content: Content) -> some View {
    if selected {
      content.neoWell(radius: MacTheme.Radius.pill)
    } else {
      content.neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
    }
  }
}

struct PaneGrid: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String?
  let mode: PaneMode?
  let splitAxis: PaneSplitAxis
  let onClose: (String) -> Void
  let onNewPane: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onTasks: () -> Void

  private var panes: [Pane] {
    guard let workspaceId else { return [] }
    return client.panes.filter { pane in
      pane.workspaceId == workspaceId && (mode == nil || pane.mode == mode)
    }
  }

  var body: some View {
    Group {
      if panes.isEmpty {
        VStack(spacing: MacTheme.Space.x18) {
          Image(systemName: mode == .editor ? "doc.text" : "rectangle.split.2x1")
            .font(.system(size: 42, weight: .light))
            .foregroundStyle(MacTheme.secondary)
          Text(mode == .editor ? "Open a file to begin" : "Your space to begin")
            .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
          if mode == .editor {
            Button("Open file", action: openEditorFile)
              .buttonStyle(NeoPillButtonStyle(accent: true))
              .disabled(!client.connected || workspaceId == nil)
          } else {
            Button("New pane", action: onNewPane).buttonStyle(NeoPillButtonStyle(accent: true))
          }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
      } else if panes.count <= 2 {
        PanePair(
          client: client, webHost: webHost, panes: panes, onClose: onClose, onTasks: onTasks,
          onSplit: onSplit, splitAxis: splitAxis, tint: paneTint)
      } else {
        PaneStrip(
          client: client, webHost: webHost, panes: panes, onClose: onClose, onTasks: onTasks,
          onSplit: onSplit, tint: paneTint)
      }
    }.padding(EdgeInsets(top: 8, leading: 6, bottom: 6, trailing: 6))
  }

  private func paneTint(_ index: Int) -> Color {
    [MacTheme.agentRose, MacTheme.agentBlue, MacTheme.agentGreen, MacTheme.agentSand][index % 4]
  }

  private func openEditorFile() {
    guard let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId }) else {
      return
    }
    let panel = NSOpenPanel()
    panel.directoryURL = URL(fileURLWithPath: workspace.path, isDirectory: true)
    panel.canChooseDirectories = false
    panel.canChooseFiles = true
    panel.allowsMultipleSelection = false
    guard panel.runModal() == .OK, let file = panel.url,
      let relative = relativePath(file, within: URL(fileURLWithPath: workspace.path))
    else { return }
    Task { @MainActor in
      do {
        _ = try await client.openEditorFile(workspaceId: workspace.workspaceId, path: relative)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func relativePath(_ file: URL, within root: URL) -> String? {
    let rootParts = root.standardizedFileURL.pathComponents
    let fileParts = file.standardizedFileURL.pathComponents
    guard fileParts.count > rootParts.count,
      Array(fileParts.prefix(rootParts.count)) == rootParts
    else { return nil }
    return fileParts.dropFirst(rootParts.count).joined(separator: "/")
  }
}

private struct PaneStrip: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panes: [Pane]
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let tint: (Int) -> Color

  private var ordered: [Pane] {
    PanePresentationOrder.arrange(panes, selectedPaneId: client.selectedPaneId)
  }

  var body: some View {
    GeometryReader { geometry in
      ScrollView(.horizontal, showsIndicators: false) {
        HStack(spacing: MacTheme.Space.x18) {
          ForEach(Array(ordered.enumerated()), id: \.element.paneId) { index, pane in
            PaneCard(
              client: client, webHost: webHost, pane: pane, tint: tint(index), onClose: onClose,
              onTasks: onTasks, onSplit: onSplit
            )
            .frame(width: paneWidth(pane, at: index, total: geometry.size.width))
            .frame(height: geometry.size.height)
          }
        }
        .frame(minWidth: geometry.size.width, alignment: .leading)
        .frame(height: geometry.size.height, alignment: .top)
      }
      .clipped()
    }
  }

  private func paneWidth(_ pane: Pane, at index: Int, total: CGFloat) -> CGFloat {
    let pair = Array(ordered.prefix(2))
    let available = max(0, total - MacTheme.Space.x18)
    if index < 2, pair.contains(where: { $0.mode == .writing }),
      pair.contains(where: { $0.mode == .agent })
    {
      return pane.mode == .writing ? available * 0.67 : available * 0.33
    }
    if index < 2, pair.contains(where: { $0.mode == .editor }),
      pair.contains(where: { $0.mode == .agent })
    {
      return pane.mode == .editor ? available * 0.7 : available * 0.3
    }
    if index < 2, Set(pair.map(\.mode)) == Set([.web, .agent]) {
      return pane.mode == .web ? available * 0.67 : available * 0.33
    }
    return pane.mode == .writing ? max(860, total * 0.67) : max(420, total * 0.42)
  }
}

private struct PanePair: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panes: [Pane]
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let splitAxis: PaneSplitAxis
  let tint: (Int) -> Color

  var body: some View {
    GeometryReader { geometry in
      splitLayout {
        ForEach(Array(ordered.enumerated()), id: \.element.paneId) { index, pane in
          PaneCard(
            client: client, webHost: webHost, pane: pane, tint: tint(index), onClose: onClose,
            onTasks: onTasks, onSplit: onSplit
          )
          .frame(
            width: splitAxis == .columns ? width(for: pane, total: geometry.size.width) : nil,
            height: splitAxis == .rows ? rowHeight(total: geometry.size.height) : nil)
        }
      }
    }
  }

  private var splitLayout: AnyLayout {
    splitAxis == .columns
      ? AnyLayout(HStackLayout(spacing: MacTheme.Space.x18))
      : AnyLayout(VStackLayout(spacing: MacTheme.Space.x18))
  }

  private func rowHeight(total: CGFloat) -> CGFloat? {
    guard ordered.count == 2 else { return nil }
    return max(0, total - MacTheme.Space.x18) / 2
  }

  private var ordered: [Pane] {
    guard panes.count == 2 else { return panes }
    if panes.contains(where: { $0.mode == .writing }) {
      return panes.sorted { left, right in left.mode == .writing && right.mode != .writing }
    }
    if panes.contains(where: { $0.mode == .editor }) {
      return panes.sorted { left, right in left.mode == .editor && right.mode != .editor }
    }
    if Set(panes.map(\.mode)) == Set([.web, .agent]) {
      return panes.sorted { left, _ in left.mode == .web }
    }
    return panes
  }

  private func width(for pane: Pane, total: CGFloat) -> CGFloat? {
    guard ordered.count == 2 else { return nil }
    let available = max(0, total - MacTheme.Space.x18)
    if ordered.contains(where: { $0.mode == .writing }) {
      return pane.mode == .writing ? available * 0.67 : available * 0.33
    }
    if ordered.contains(where: { $0.mode == .editor }) {
      return pane.mode == .editor ? available * 0.7 : available * 0.3
    }
    if Set(ordered.map(\.mode)) == Set([.web, .agent]) {
      return pane.mode == .web ? available * 0.67 : available * 0.33
    }
    return nil
  }
}

private struct EmptyWorkspaceView: View {
  let icon: String
  let title: String

  var body: some View {
    VStack(spacing: MacTheme.Space.x18) {
      Image(systemName: icon)
        .font(.system(size: 42, weight: .light))
        .foregroundStyle(MacTheme.secondary)
      Text(title)
        .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }
}

private struct PaneCard: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let pane: Pane
  let tint: Color
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  @State private var showingAutomation = false
  @State private var showingHarnessSwitch = false

  var body: some View {
    Group {
      if pane.mode == .editor {
        EditorPaneView(client: client, pane: pane)
      } else if pane.mode == .writing, pane.writingKind == .text {
        WritingTextPane(client: client, pane: pane)
      } else if pane.mode == .writing, pane.writingKind == .presentation {
        WritingPresentationPane(client: client, pane: pane)
      } else if pane.mode == .writing, pane.writingKind == .screenplay {
        WritingScreenplayPane(client: client, pane: pane)
      } else {
        VStack(spacing: MacTheme.Space.x8) {
          header
          paneBody
        }
        .padding(MacTheme.Space.x10).neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
      }
    }
    .contentShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .simultaneousGesture(TapGesture().onEnded { client.select(pane.paneId) })
    .contextMenu { Button("Close pane", role: .destructive) { onClose(pane.paneId) } }
    .overlay(alignment: .topTrailing) {
      if showingAutomation {
        PaneAutomationPanel(client: client, pane: pane) { showingAutomation = false }
          .offset(x: -6, y: 38)
          .transition(.scale(scale: 0.98, anchor: .topTrailing).combined(with: .opacity))
          .zIndex(20)
      }
    }
    .zIndex(showingAutomation ? 20 : 0)
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x7) {
      Circle().fill(stateColor).frame(width: 7, height: 7)
      Text(pane.name).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold)).lineLimit(1)
      if pane.mode == .agent, let summary = agentSummary {
        Text(summary).font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
          .foregroundStyle(MacTheme.secondary).lineLimit(1)
      }
      Spacer(minLength: 0)
      if pane.mode == .agent {
        Text(pane.state.rawValue.replacingOccurrences(of: "_", with: " "))
          .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .semibold))
          .foregroundStyle(MacTheme.secondary)
      }
      if pane.mode == .agent {
        Button {
          showingHarnessSwitch = true
        } label: {
          Image(systemName: "arrow.triangle.2.circlepath")
        }
        .help("Switch agent")
        .accessibilityLabel("Switch agent")
        .accessibilityIdentifier("switch-agent-\(pane.paneId)")
        .disabled(!client.connected || pane.agent?.operation != nil || harnessSwitching)
        .popover(isPresented: $showingHarnessSwitch, arrowEdge: .bottom) {
          AgentHarnessSwitchPicker(
            client: client, pane: pane, isPresented: $showingHarnessSwitch)
        }
      }
      if pane.mode == .agent { PaneAutomationButton(showing: $showingAutomation) }
      Button(action: onTasks) { Image(systemName: "line.3.horizontal.decrease") }
        .help("Open workspace tasks")
      Button {
        onSplit(pane.paneId, .columns)
      } label: {
        Image(systemName: "rectangle.split.2x1")
      }
      .help("Split pane into columns")
      .accessibilityLabel("Split pane into columns")
      .accessibilityIdentifier("split-pane-columns-\(pane.paneId)")
      .disabled(!client.connected)
      Button {
        onSplit(pane.paneId, .rows)
      } label: {
        Image(systemName: "rectangle.split.1x2")
      }
      .help("Split pane into rows")
      .accessibilityLabel("Split pane into rows")
      .accessibilityIdentifier("split-pane-rows-\(pane.paneId)")
      .disabled(!client.connected)
      Button {
        onClose(pane.paneId)
      } label: {
        Image(systemName: "xmark")
      }
      .help("Close this pane").disabled(!client.connected)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 26)
    .background(tint, in: Capsule())
  }

  @ViewBuilder private var paneBody: some View {
    if pane.mode == .web {
      WebPaneSurface(client: client, host: webHost, pane: pane)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    } else if pane.mode == .agent {
      VStack(spacing: MacTheme.Space.x12) {
        if let task = client.tasks.first(where: { $0.ownerPaneId == pane.paneId }) {
          HStack(spacing: MacTheme.Space.x8) {
            Image(systemName: "list.bullet.clipboard").foregroundStyle(MacTheme.accent)
            Text(task.taskId).font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
              .foregroundStyle(MacTheme.muted)
            Text(task.title).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
              .lineLimit(1)
            Spacer()
            if let branch = task.worktree?.branch {
              Label(branch, systemImage: "arrow.triangle.branch")
                .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
                .foregroundStyle(MacTheme.muted)
            }
            Image(systemName: "chevron.right").foregroundStyle(MacTheme.muted)
          }
          .padding(.horizontal, MacTheme.Space.x12).frame(height: 34)
          .neoRaised(radius: MacTheme.Radius.control, elevation: .chip)
        }
        AgentConversationView(
          client: client, paneId: pane.paneId, showsConfiguration: false,
          style: MacTheme.agentConversation)
      }.padding(EdgeInsets(top: 4, leading: 4, bottom: 0, trailing: 4))
    } else if client.selectedPaneId == pane.paneId {
      VStack(spacing: 0) {
        MacTerminal(client: client)
        if let gap = client.gapText {
          Label(gap, systemImage: "exclamationmark.triangle")
            .font(MacTheme.chrome(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.warning)
            .padding(MacTheme.Space.x10)
        }
      }
    } else {
      VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
        Text(pane.phrase).font(MacTheme.mono(MacTheme.TypeScale.caption))
        Spacer()
        Text(pane.cwd).font(MacTheme.mono(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.muted).lineLimit(1).truncationMode(.middle)
      }.padding(EdgeInsets(top: 11, leading: 12, bottom: 11, trailing: 12))
    }
  }

  private var stateColor: Color {
    switch pane.state {
    case .working, .idle: MacTheme.success
    case .waiting_for_you: MacTheme.warning
    case .dead, .no_signal: MacTheme.danger
    default: MacTheme.ink.opacity(0.75)
    }
  }

  private var agentSummary: String? {
    guard let agent = pane.agent else { return nil }
    let values = agent.options.compactMap { option -> String? in
      guard option.category == "model" || option.category == "thought_level" else { return nil }
      if case .v0(let value) = option.currentValue { return value }
      return nil
    }
    return ([agent.harness] + values).joined(separator: " · ")
  }

  private var harnessSwitching: Bool {
    client.agentPanes[pane.paneId]?.harnessSwitch?.phase == .switching
  }
}

private struct NewPaneWorkspace: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String?
  let splitAxis: PaneSplitAxis
  @Binding var isPresented: Bool
  let onClose: (String) -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onTasks: () -> Void

  var body: some View {
    splitLayout {
      if let pane = client.selectedPane {
        PaneCard(
          client: client, webHost: webHost, pane: pane, tint: MacTheme.agentRose, onClose: onClose,
          onTasks: onTasks, onSplit: onSplit
        )
        .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
      NewPaneSheet(client: client, workspaceId: workspaceId, isPresented: $isPresented)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }.padding(EdgeInsets(top: 8, leading: 6, bottom: 6, trailing: 6))
  }

  private var splitLayout: AnyLayout {
    splitAxis == .columns
      ? AnyLayout(HStackLayout(spacing: MacTheme.Space.x18))
      : AnyLayout(VStackLayout(spacing: MacTheme.Space.x18))
  }
}

private struct NewPaneSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  @Binding var isPresented: Bool
  @State private var mode = PaneMode.terminal
  @State private var directory: URL?
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var permission = PermissionMode.ask
  @State private var showsAgentPicker = false
  @State private var submitting = false
  @State private var prompt = ""
  @FocusState private var promptFocused: Bool

  private var selectedHarness: HarnessInfo? { client.harnesses.first { $0.id == harness } }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first { $0.category == "thought_level" || $0.id == "effort" }
  }

  var body: some View {
    VStack(spacing: MacTheme.Space.x8) {
      HStack(spacing: MacTheme.Space.x7) {
        Circle().fill(MacTheme.muted).frame(width: 7, height: 7)
        Text("new pane").font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
        Spacer()
        Image(systemName: "rectangle.split.2x1").accessibilityHidden(true)
        Image(systemName: "rectangle.split.1x2").accessibilityHidden(true)
        Button {
          isPresented = false
        } label: {
          Image(systemName: "xmark")
        }
        .help("Close new pane")
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.secondary)
      .padding(.horizontal, MacTheme.Space.x12).frame(height: 26)
      .background(MacTheme.well, in: Capsule())

      VStack(spacing: MacTheme.Space.x18) {
        Text("What do you want to do?")
          .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
        VStack(spacing: 0) {
          if mode == .agent {
            ZStack(alignment: .topLeading) {
              TextField("", text: $prompt, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...4)
                .font(MacTheme.chrome(MacTheme.TypeScale.body))
                .foregroundStyle(MacTheme.ink)
                .focused($promptFocused)
                .padding(.trailing, 54)
                .accessibilityLabel("Ask anything")
                .accessibilityHint("Type the first message for the new agent")
              if prompt.isEmpty {
                Text("Ask anything. Choose a directory and an agent below.")
                  .font(MacTheme.chrome(MacTheme.TypeScale.body))
                  .foregroundStyle(MacTheme.muted)
                  .allowsHitTesting(false)
              }
              HStack {
                Spacer()
                Button("⌘⇧C") { promptFocused = true }
                  .buttonStyle(.plain)
                  .keyboardShortcut("c", modifiers: [.command, .shift])
                  .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
                  .foregroundStyle(MacTheme.muted)
                  .accessibilityLabel("Focus new agent message")
              }
            }
            .contentShape(Rectangle())
            .onTapGesture { promptFocused = true }
          } else {
            HStack(alignment: .top) {
              Text("Open a terminal in the directory selected below.")
                .font(MacTheme.chrome(MacTheme.TypeScale.body))
                .foregroundStyle(MacTheme.muted)
              Spacer()
            }
            Spacer()
          }
          HStack(spacing: MacTheme.Space.x8) {
            Button(action: chooseDirectory) {
              Image(systemName: "plus").font(.system(size: 16))
            }.help("Choose a directory")
            if mode == .agent {
              Button {
                showsAgentPicker.toggle()
              } label: {
                HStack(spacing: MacTheme.Space.x6) {
                  Image(systemName: "cpu")
                  Text(selectedAgentLabel)
                    .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
                  if !effort.isEmpty {
                    Text("·").foregroundStyle(MacTheme.muted)
                    Text(effort.capitalized)
                      .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
                      .foregroundStyle(MacTheme.secondary)
                  }
                  Image(systemName: "chevron.down").font(.system(size: 10))
                    .foregroundStyle(MacTheme.muted)
                }
              }
              .popover(isPresented: $showsAgentPicker, arrowEdge: .bottom) { agentPicker }
            } else {
              Button(action: create) {
                Text("Terminal")
                  .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
              }
              .buttonStyle(.plain)
              .disabled(directory == nil || submitting)
              .accessibilityHint("Open a terminal in the selected directory")
            }
            Spacer()
            Button(action: create) {
              Image(systemName: "arrow.up").font(.system(size: 14, weight: .bold))
                .foregroundStyle(MacTheme.onAccent).frame(width: 30, height: 30)
                .neoAccent(radius: MacTheme.Radius.pill)
            }
            .disabled(
              directory == nil || (mode == .agent && selectedHarness?.available != true)
                || prompt.utf8.count > ProtocolLimits.promptBytes || submitting
            )
            .keyboardShortcut(.return, modifiers: [.command, .shift])
          }
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 12, trailing: 16))
        .frame(width: 520, height: 150).neoWell(radius: MacTheme.Radius.control)

        metadata
        HStack(spacing: MacTheme.Space.x10) {
          Text(mode == .agent ? "Prefer the command line?" : "Prefer a structured agent?")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
          Button {
            mode = mode == .agent ? .terminal : .agent
            if mode == .agent {
              showsAgentPicker = true
              promptFocused = true
            }
          } label: {
            Label(
              mode == .agent ? "Open a terminal" : "Open an agent",
              systemImage: mode == .agent ? "terminal" : "cpu")
          }.buttonStyle(NeoPillButtonStyle())
          Text("⌘⇧Enter")
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }.frame(width: 520, alignment: .leading).padding(.top, MacTheme.Space.x14)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .offset(y: -16)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
    .padding(MacTheme.Space.x10).neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .onAppear {
      client.refreshHarnesses(rescan: true)
      selectFirstHarness()
      selectWorkspaceDirectory()
    }
    .onChange(
      of: client.harnesses.map {
        "\($0.id):\($0.available):\(HarnessPickerModel.statusLabel(for: $0))"
      }
    ) { _, _ in selectFirstHarness() }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      if selectedHarness?.available == true { client.refreshCatalogue(harness: value) }
    }
  }

  private var metadata: some View {
    HStack(spacing: MacTheme.Space.x14) {
      Button(action: chooseDirectory) {
        Label(
          directory?.lastPathComponent ?? "Project", systemImage: "folder"
        )
        .lineLimit(1)
      }
      Label("Local", systemImage: "desktopcomputer")
      Label("main", systemImage: "arrow.triangle.branch")
      Spacer()
      if mode == .agent {
        Menu {
          Button("Ask every time") { permission = .ask }
          Button("Accept edits") { permission = .accept_edits }
          Button("Allow all") { permission = .allow_all }
        } label: {
          Label(permissionLabel, systemImage: "shield.checkered")
        }.menuStyle(.borderlessButton).fixedSize()
      }
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .foregroundStyle(MacTheme.secondary).frame(width: 508)
  }

  private var agentPicker: some View {
    AgentHarnessPicker(
      client: client, harness: $harness, model: $model, effort: $effort,
      catalogue: catalogue, effortOption: effortOption, excludingHarness: nil)
  }

  private var selectedAgentLabel: String {
    let modelName = catalogue?.models.first { $0.id == model }?.name
    return modelName ?? selectedHarness?.name ?? "Choose agent"
  }

  private var permissionLabel: String {
    switch permission {
    case .ask: "Ask"
    case .accept_edits: "Edits"
    case .allow_all: "Allow all"
    }
  }

  private func selectFirstHarness() {
    guard selectedHarness?.available != true,
      let first = HarnessPickerModel.defaultHarness(in: client.harnesses)
    else { return }
    harness = first.id
  }

  private func chooseDirectory() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.prompt = "Choose"
    panel.begin { response in
      if response == .OK { directory = panel.url }
    }
  }

  private func selectWorkspaceDirectory() {
    guard directory == nil,
      let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return }
    directory = URL(fileURLWithPath: workspace.path, isDirectory: true)
  }

  private func create() {
    guard let directory, !submitting else { return }
    let firstPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard firstPrompt.utf8.count <= ProtocolLimits.promptBytes else { return }
    submitting = true
    Task { @MainActor in
      do {
        if mode == .terminal {
          _ = try await client.createTerminalPane(directory: directory)
        } else {
          let paneId = try await client.createAgentPaneNow(
            directory: directory, harness: harness, model: model.isEmpty ? nil : model,
            effort: effort.isEmpty ? nil : effort, permission: permission)
          if !firstPrompt.isEmpty {
            try await client.sendPromptNow(paneId, text: firstPrompt)
          }
        }
        isPresented = false
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct AgentHarnessPicker: View {
  @Bindable var client: EngineClient
  @Binding var harness: String
  @Binding var model: String
  @Binding var effort: String
  let catalogue: AgentCatalogue?
  let effortOption: ConfigOption?
  let excludingHarness: String?
  @State private var query = ""

  private var sections: [HarnessPickerSection] {
    HarnessPickerModel.sections(
      client.harnesses.filter { $0.id != excludingHarness }, query: query)
  }

  private var selected: HarnessInfo? { client.harnesses.first { $0.id == harness } }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      HStack(spacing: MacTheme.Space.x8) {
        TextField("Search agents", text: $query)
          .textFieldStyle(.plain)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .padding(.horizontal, MacTheme.Space.x10).frame(height: 32)
          .neoWell(radius: MacTheme.Radius.compact)
        Button {
          client.refreshHarnesses(rescan: true)
        } label: {
          Image(systemName: "arrow.clockwise")
        }
        .help("Rescan installed agents")
        .accessibilityLabel("Rescan installed agents")
      }
      if sections.isEmpty {
        ContentUnavailableView.search(text: query)
          .frame(maxWidth: .infinity, minHeight: 220)
      } else {
        ScrollView {
          LazyVStack(alignment: .leading, spacing: MacTheme.Space.x12) {
            ForEach(sections, id: \.group.rawValue) { section in
              Text(section.group.title.uppercased()).pickerHeading()
              ForEach(section.items, id: \.id) { item in harnessRow(item) }
            }
          }
          .padding(.trailing, MacTheme.Space.x4)
        }
        .frame(height: 260)
      }
      Divider().overlay(MacTheme.border)
      selectedConfiguration
    }
    .padding(EdgeInsets(top: 14, leading: 14, bottom: 14, trailing: 14))
    .frame(width: 420, height: 510, alignment: .top)
    .foregroundStyle(MacTheme.ink)
    .background(MacTheme.face)
  }

  private func harnessRow(_ item: HarnessInfo) -> some View {
    let status = HarnessPickerModel.setupState(for: item)
    return Button {
      harness = item.id
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: statusSymbol(status))
          .foregroundStyle(statusColor(status)).frame(width: 18)
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(item.name)
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold)).lineLimit(1)
          Text(HarnessPickerModel.statusLabel(for: item))
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
        Spacer()
        if item.id == harness { Image(systemName: "checkmark").foregroundStyle(MacTheme.accent) }
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 42)
      .background(
        item.id == harness ? MacTheme.well : .clear,
        in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(item.name), \(HarnessPickerModel.statusLabel(for: item))")
    .accessibilityIdentifier("agent-choice-\(item.id)")
  }

  @ViewBuilder private var selectedConfiguration: some View {
    if let selected, selected.available {
      Text("MODEL").pickerHeading()
      Menu {
        Button("Agent default") { model = "" }
        ForEach(catalogue?.models ?? [], id: \.id) { item in
          Button(item.name) { model = item.id }
        }
      } label: {
        HStack {
          Text(selectedModelLabel)
          Spacer()
          Image(systemName: "chevron.up.chevron.down")
            .font(.system(size: 10)).foregroundStyle(MacTheme.muted)
        }
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
        .neoWell(radius: MacTheme.Radius.compact)
      }
      .menuStyle(.borderlessButton)
      if let choices = effortOption?.options, !choices.isEmpty {
        Text("EFFORT").pickerHeading()
        HStack(spacing: MacTheme.Space.x2) {
          ForEach(choices, id: \.value) { choice in
            Button(choice.name) { effort = choice.value }
              .font(
                MacTheme.chrome(
                  MacTheme.TypeScale.control, weight: effort == choice.value ? .bold : .semibold)
              )
              .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
              .background(effort == choice.value ? MacTheme.face : .clear, in: Capsule())
          }
        }.padding(3).background(MacTheme.well, in: Capsule())
      }
    } else if let selected {
      Label(
        "\(selected.name) is not installed.",
        systemImage: "exclamationmark.circle"
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
      .foregroundStyle(MacTheme.muted)
    }
  }

  private var selectedModelLabel: String {
    catalogue?.models.first { $0.id == model }?.name ?? "Agent default"
  }

  private func statusSymbol(_ status: HarnessSetupState) -> String {
    switch status {
    case .ready: "checkmark.circle.fill"
    case .sign_in: "person.crop.circle.badge.exclamationmark"
    case .check_setup: "wrench.and.screwdriver"
    case .not_installed: "minus.circle"
    }
  }

  private func statusColor(_ status: HarnessSetupState) -> Color {
    switch status {
    case .ready: MacTheme.success
    case .sign_in, .check_setup: MacTheme.warning
    case .not_installed: MacTheme.muted
    }
  }
}

private struct AgentHarnessSwitchPicker: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @Binding var isPresented: Bool
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var submitting = false

  private var currentHarness: String { pane.agent?.harness ?? "" }
  private var selectedHarness: HarnessInfo? { client.harnesses.first { $0.id == harness } }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first { $0.category == "thought_level" || $0.id == "effort" }
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        Text("Switch agent")
          .font(MacTheme.chrome(MacTheme.TypeScale.body, weight: .bold))
        Text(
          "This starts a new agent session. Recent messages and pending prompts are handed "
            + "over as historical data."
        )
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.muted)
        .fixedSize(horizontal: false, vertical: true)
      }
      .padding(.horizontal, MacTheme.Space.x14)
      .padding(.top, MacTheme.Space.x14)
      AgentHarnessPicker(
        client: client, harness: $harness, model: $model, effort: $effort,
        catalogue: catalogue, effortOption: effortOption, excludingHarness: currentHarness)
      Button(action: switchHarness) {
        Text(actionLabel)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(NeoPillButtonStyle(accent: true))
      .disabled(selectedHarness?.available != true || submitting)
      .padding(.horizontal, MacTheme.Space.x14)
      .padding(.bottom, MacTheme.Space.x14)
    }
    .frame(width: 448)
    .background(MacTheme.face)
    .onAppear {
      client.refreshHarnesses(rescan: false)
      selectFirstHarness()
    }
    .onChange(
      of: client.harnesses.map {
        "\($0.id):\($0.available):\(HarnessPickerModel.statusLabel(for: $0))"
      }
    ) { _, _ in selectFirstHarness() }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      if selectedHarness?.available == true { client.refreshCatalogue(harness: value) }
    }
  }

  private var actionLabel: String {
    guard let selectedHarness else { return "Choose another agent" }
    return "Restart conversation with \(selectedHarness.name)"
  }

  private func selectFirstHarness() {
    guard selectedHarness?.available != true,
      let first = HarnessPickerModel.defaultHarness(
        in: client.harnesses, excluding: currentHarness)
    else { return }
    harness = first.id
  }

  private func switchHarness() {
    guard selectedHarness?.available == true, !submitting else { return }
    submitting = true
    Task { @MainActor in
      do {
        _ = try await client.switchAgentHarness(
          pane.paneId, harness: harness, model: model.isEmpty ? nil : model,
          effort: effort.isEmpty ? nil : effort)
        submitting = false
        isPresented = false
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}
