import AppKit
import CoreImage.CIFilterBuiltins
import Observation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WorkspaceWindow: View {
  @Environment(AgentAttentionNotifications.self) private var attentionNotifications
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  @State private var navigation = WorkspaceNavigation()
  @State private var webLinkPreferences = WebLinkPreferences()
  @State private var paneToClose: String?
  @State private var newPaneDraft: NewPaneDraft?
  @State private var showPairing = false
  @State private var deviceToRevoke: String?
  @State private var taskWorkspaceId: String?
  @State private var showPreferences = false
  @State private var selectedUsageProvider: UsageProvider?
  @State private var editorPaletteMode: EditorPaletteMode?
  @State private var slideShow = MacWritingSlideShowCoordinator()
  var body: some View {
    GeometryReader { geometry in
      workspaceChrome(windowSize: geometry.size)
        .environment(
          \.macSheetAvailableSize,
          PaneLayoutSize(width: geometry.size.width, height: geometry.size.height))
    }
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink).buttonStyle(.plain)
    .environment(webLinkPreferences)
    .environment(
      \.terminalKeyboardInputEnabled, editorPaletteMode == nil && !slideShow.isPresenting
    )
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
    .onAppear {
      webHost.reconcile(client.panes)
      attentionNotifications.observe(
        client.agentAttention, workspaces: client.workspaces, connected: client.connected)
      openPendingAgentAttention()
    }
    .onChange(of: client.panes.map(\.paneId)) { _, _ in webHost.reconcile(client.panes) }
    .onChange(of: client.connectionRevision) { _, _ in webHost.resynchronize() }
    .onChange(of: client.agentAttention) { _, summary in
      attentionNotifications.observe(
        summary, workspaces: client.workspaces, connected: client.connected)
    }
    .onChange(of: client.connected) { _, _ in
      attentionNotifications.observe(
        client.agentAttention, workspaces: client.workspaces, connected: client.connected)
      openPendingAgentAttention()
    }
    .onChange(of: attentionNotifications.pendingRoute) { _, _ in openPendingAgentAttention() }
    .onExitCommand { showPreferences = false }
    .modifier(WritingSlideShowHostModifier(client: client, coordinator: slideShow))
  }

  private func workspaceChrome(windowSize: CGSize) -> some View {
    let policy = MacPaneLayoutMetrics.window
    let paneViewport = policy.paneViewport(
      window: PaneLayoutSize(width: windowSize.width, height: windowSize.height),
      railOpen: navigation.appBarOpen)
    let horizontalInset = policy.horizontalInset(windowWidth: windowSize.width)
    let rowWidth = max(0, windowSize.width - 2 * horizontalInset)
    let placement = policy.horizontalPlacement(
      windowWidth: windowSize.width, railOpen: navigation.appBarOpen)

    return VStack(spacing: CGFloat(policy.paneVerticalInset)) {
      WorkspaceTopBar(
        client: client, workspaceId: selectedTaskWorkspaceId,
        onSelectWorkspace: selectWorkspace, onCloseWorkspace: closeWorkspace,
        onNewPane: presentNewPane,
        onTasks: showTasks, onToggleAppBar: { navigation.toggleAppBar() })
      ZStack(alignment: .leading) {
        workspaceContent(
          viewportSize: CGSize(width: paneViewport.width, height: paneViewport.height)
        )
        .frame(width: paneViewport.width, height: paneViewport.height)
        .offset(x: placement.paneLeading - horizontalInset)
        if navigation.appBarOpen {
          appRail
            .frame(width: MacPaneLayoutMetrics.railWidth, height: paneViewport.height)
            .background(placement.railOverlays ? MacTheme.canvas : Color.clear)  // halo shows
            .offset(x: placement.railLeading - horizontalInset)
            .transition(.move(edge: .leading).combined(with: .opacity))
            .zIndex(30)
        }
      }
      .frame(width: rowWidth, height: paneViewport.height, alignment: .leading)
    }
    .padding(.top, CGFloat(policy.topInset))
    .padding(.bottom, CGFloat(policy.paneVerticalInset))
    .padding(.horizontal, CGFloat(horizontalInset))
    .frame(width: windowSize.width, height: windowSize.height)
  }

  private var appRail: some View {
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
        client.refreshUsageOnDetailsClick(provider)
      })
  }

  @ViewBuilder private func workspaceContent(viewportSize: CGSize) -> some View {
    switch navigation.destination {
    case .tasks:
      if let workspaceId = selectedTaskWorkspaceId {
        TaskWorkspaceView(client: client, workspaceId: workspaceId, onOpenPane: openPane)
      } else {
        EmptyWorkspaceView(icon: "checklist", title: "No workspace selected")
      }
    case .agents:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: nil,
        viewportSize: viewportSize, layoutPersistenceEnabled: !slideShow.isPresenting,
        newPane: newPanePresentation,
        onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .terminal:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .terminal,
        viewportSize: viewportSize, layoutPersistenceEnabled: !slideShow.isPresenting,
        onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .editor:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .editor,
        viewportSize: viewportSize, layoutPersistenceEnabled: !slideShow.isPresenting,
        onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .web:
      WebWorkspace(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId,
        viewportSize: viewportSize,
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
    newPaneDraft = nil
    taskWorkspaceId = workspaceId
    if let pane = client.panes.first(where: { $0.workspaceId == workspaceId }) {
      client.select(pane.paneId)
    }
  }
  private func closeWorkspace(_ workspaceId: String) {
    let order = client.workspaces.map(\.workspaceId)
    let wasSelected = selectedTaskWorkspaceId == workspaceId
    let replacement = WorkspaceTabStripPolicy.selectionAfterClosing(
      workspaceId: workspaceId, orderedWorkspaceIds: order)
    if wasSelected {
      if let replacement {
        selectWorkspace(replacement)
      } else {
        taskWorkspaceId = nil
        client.select(nil)
      }
    }
    Task { @MainActor in
      do {
        try await client.closeWorkspace(workspaceId)
      } catch {
        if wasSelected, client.workspaces.contains(where: { $0.workspaceId == workspaceId }) {
          selectWorkspace(workspaceId)
        }
        client.problem = "Couldn't close the workspace. It is still available."
      }
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
    do {
      let workspaceId = selectedTaskWorkspaceId
      let snapshot = try workspaceId.map(layoutSnapshot)
      newPaneDraft = NewPaneDraft(
        workspaceId: workspaceId,
        insertion: PaneLayoutProvisionalInsertion(
          baseTree: snapshot?.state.tree,
          placeholder: PaneLayoutLeaf(paneId: newPanePlaceholderId(), kind: .terminal),
          placement: .arranged(rule: snapshot?.state.insertionRule ?? .initialByType)))
      client.refreshHarnesses()
      navigation.select(.agents)
    } catch {
      client.problem = "Couldn't open the new pane chooser. The layout was not changed."
    }
  }
  private func presentSplit(_ paneId: String, _ axis: PaneSplitAxis) {
    guard let anchor = client.panes.first(where: { $0.paneId == paneId }) else { return }
    do {
      let snapshot = try layoutSnapshot(anchor.workspaceId)
      newPaneDraft = NewPaneDraft(
        workspaceId: anchor.workspaceId,
        insertion: PaneLayoutProvisionalInsertion(
          baseTree: snapshot.state.tree,
          placeholder: PaneLayoutLeaf(paneId: newPanePlaceholderId(), kind: .terminal),
          placement: .split(
            anchorPaneId: paneId, axis: axis == .columns ? .horizontal : .vertical)))
      client.select(paneId)
      client.refreshHarnesses()
      navigation.select(.agents)
    } catch {
      client.problem = "Couldn't open the split chooser. The layout was not changed."
    }
  }
  private func completeNewPane(_ newPaneId: String, _ kind: PaneLayoutKind) {
    guard let draft = newPaneDraft else { return }
    defer { newPaneDraft = nil }
    guard case .split = draft.insertion.placement, let workspaceId = draft.workspaceId else {
      return
    }
    do {
      let companionWritingPaneId: String?
      if kind == .agent, case .split(let anchorPaneId, _) = draft.insertion.placement,
        client.panes.first(where: { $0.paneId == anchorPaneId })?.mode == .writing
      {
        companionWritingPaneId = anchorPaneId
      } else {
        companionWritingPaneId = nil
      }
      try client.paneLayouts.commit(
        tree: draft.completedTree(
          paneId: newPaneId, kind: kind,
          companionWritingPaneId: companionWritingPaneId),
        workspaceId: workspaceId)
    } catch {
      client.problem = "Couldn't split the pane. The new pane is still available."
    }
  }
  private var newPanePresentation: NewPanePlaceholderPresentation? {
    guard let draft = newPaneDraft else { return nil }
    return NewPanePlaceholderPresentation(
      draft: draft,
      onKindChange: { kind in
        guard newPaneDraft?.placeholderPaneId == draft.placeholderPaneId else { return }
        newPaneDraft?.select(kind: kind)
      },
      onCancel: { newPaneDraft = nil },
      onCreated: completeNewPane)
  }
  private func layoutSnapshot(_ workspaceId: String) throws -> PaneLayoutReconciliation {
    try client.paneLayouts.reconciledPresentation(
      for: workspaceId,
      livePanes: client.layoutPanes(client.panes.filter { $0.workspaceId == workspaceId }),
      sizing: MacPaneLayoutMetrics.sizing)
  }
  private func newPanePlaceholderId() -> String {
    "local-new-pane-\(UUID().uuidString.lowercased())"
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

  private func openPendingAgentAttention() {
    guard let route = attentionNotifications.pendingRoute else { return }
    guard client.connected else { return }
    guard
      client.panes.contains(where: {
        $0.paneId == route.paneId && $0.workspaceId == route.workspaceId && $0.mode == .agent
      })
    else {
      attentionNotifications.clearPendingRoute()
      return
    }
    guard
      client.agentAttention.requests.contains(where: {
        $0.identity == route.requestIdentity
      })
    else {
      attentionNotifications.clearPendingRoute()
      return
    }
    taskWorkspaceId = route.workspaceId
    navigation.select(.agents)
    client.select(route.paneId)
    NSApp.activate(ignoringOtherApps: true)
    NSApp.windows.first(where: \.isVisible)?.makeKeyAndOrderFront(nil)
    attentionNotifications.clearPendingRoute()
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
    if destination != .agents { newPaneDraft = nil }
  }
  private var pairing: some View {
    ScrollView { pairingContent }.macSheetSurface(.pairing).background(MacTheme.canvas)
  }
  private var pairingContent: some View {
    VStack(spacing: 20) {
      Text("Continue on iPhone").font(MacTheme.chrome(24, weight: .semibold))
      let guidance = client.pairingGuidance
      Text(guidance.text)
        .multilineTextAlignment(.center)
        .foregroundStyle(guidance.isProblem ? MacTheme.danger : MacTheme.secondary)
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
    }.padding(32).frame(maxWidth: .infinity).background(MacTheme.canvas).foregroundStyle(
      MacTheme.ink
    )
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
      .frame(maxWidth: 760, alignment: .leading)
      if client.devices.isEmpty {
        EmptyWorkspaceView(icon: "iphone", title: "No paired iPhone")
      } else {
        VStack(spacing: MacTheme.Space.x10) {
          ForEach(client.devices, id: \.deviceId) { device in
            PairedPhoneRow(device: device)
          }
        }
        .frame(maxWidth: 760, alignment: .leading)
      }
    }
    .padding(EdgeInsets(top: 8, leading: 14, bottom: 6, trailing: 6))
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
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

private struct PairedPhoneRow: View {
  let device: Device
  @State private var showingFingerprint = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x12) {
      Image(systemName: "iphone")
        .font(.system(size: 18, weight: .medium))
        .foregroundStyle(MacTheme.accent)
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text(device.name)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        Button {
          showingFingerprint = true
        } label: {
          HStack(spacing: MacTheme.Space.x4) {
            Text(compactFingerprint)
              .font(MacTheme.mono(MacTheme.TypeScale.small))
            Image(systemName: "ellipsis.circle")
              .font(.system(size: MacTheme.TypeScale.small, weight: .medium))
          }
          .foregroundStyle(MacTheme.secondary)
        }
        .buttonStyle(.plain)
        .help("Show full device fingerprint")
        .accessibilityLabel("Show full fingerprint for \(device.name)")
        .popover(isPresented: $showingFingerprint, arrowEdge: .bottom) {
          VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
            Text("Device fingerprint")
              .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
            Text(device.fingerprint)
              .font(MacTheme.mono(MacTheme.TypeScale.small))
              .textSelection(.enabled)
          }
          .padding(MacTheme.Space.x16)
          .frame(width: 520, alignment: .leading)
        }
      }
      Spacer()
      Text(device.revoked ? "Revoked" : "Paired")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        .foregroundStyle(device.revoked ? MacTheme.danger : MacTheme.success)
    }
    .padding(MacTheme.Space.x14)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .chip)
  }

  private var compactFingerprint: String {
    guard device.fingerprint.count > 17 else { return device.fingerprint }
    return "\(device.fingerprint.prefix(8))…\(device.fingerprint.suffix(8))"
  }
}

struct AgentAttentionBadge: View {
  let count: Int

  var body: some View {
    Text(count, format: .number)
      .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .foregroundStyle(MacTheme.ink)
      .padding(.horizontal, MacTheme.Space.x6)
      .padding(.vertical, MacTheme.Space.x2)
      .background(MacTheme.warning, in: Capsule())
      .accessibilityLabel(AgentAttentionPolicy.waitingLabel(count: count))
  }
}

struct AgentWorkingDot: View {
  var body: some View {
    Circle().fill(MacTheme.accent)
      .frame(width: MacTheme.Space.x7, height: MacTheme.Space.x7)
      .accessibilityLabel("Agents working")
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
