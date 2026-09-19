import AppKit
import CoreImage.CIFilterBuiltins
import CoreText
import OSLog
import Observation
import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UniformTypeIdentifiers

enum MacPaneLayoutMetrics {
  /// Named from the smallest width and height exercised by the accepted floor report.
  static let minimumExercisedViewport = PaneLayoutSize(width: 375, height: 375)
  static let topBarHeight = 42.0
  static let workspaceTabHeight = 36.0
  static let topBarControlHeight = 34.0
  static let paneVerticalInset = 17.0
  static let railWidth = 56.0

  static let paneHalo: Double = {
    let elevation = MacTheme.Elevation.pane
    return Double(
      max(
        abs(elevation.lightOffset) + elevation.lightBlur,
        abs(elevation.darkOffset) + elevation.darkBlur))
  }()
  static let workspaceTabShadowClearance: Double = {
    let elevation = MacTheme.Elevation.pill
    return Double(
      max(
        elevation.lightOffset + elevation.lightBlur,
        elevation.darkOffset + elevation.darkBlur))
  }()
  static let workspaceTabViewportHeight =
    workspaceTabHeight + 2 * workspaceTabShadowClearance

  static let surfaceFloors = PaneLayoutFloorFormula.surfaceFloors(fonts: fontMetrics)
  static let footprintFloors = PaneLayoutFloorFormula.footprintFloors(
    fonts: fontMetrics, halo: paneHalo)
  static let newPaneSurfaceFloor = PaneLayoutFloorFormula.newPaneChooserSurfaceFloor(
    fonts: fontMetrics)
  static let newPaneFootprintFloor = PaneLayoutSize(
    width: ceil(newPaneSurfaceFloor.width + 2 * paneHalo),
    height: ceil(newPaneSurfaceFloor.height + 2 * paneHalo))
  static let sizing = PaneLayoutSizingPolicy(
    floors: footprintFloors,
    gaps: PaneLayoutGaps(
      horizontal: Double(MacTheme.Space.x18), vertical: Double(MacTheme.Space.x18)))
  static func sizing(newPanePlaceholderId: String) -> PaneLayoutSizingPolicy {
    sizing.overridingFloor(forPaneId: newPanePlaceholderId, with: newPaneFootprintFloor)
  }
  static let window = PaneLayoutWindowPolicy(
    minimumExercisedViewport: minimumExercisedViewport,
    largestPaneFootprintWidth: footprintFloors.values.map(\.width).max() ?? 0,
    standardHorizontalInset: Double(MacTheme.Space.x18),
    topInset: Double(MacTheme.Space.x8), topBarHeight: topBarHeight,
    paneVerticalInset: paneVerticalInset, railWidth: railWidth)

  private static let fontMetrics = PaneLayoutFontMetrics(
    terminal: metric(
      MacTerminalFont.active(),
      character: "W",
      snapToDisplayPixels: true),
    editor: metric(
      font(named: "JetBrains Mono", size: PaneLayoutContentMetrics.editorFontPointSize),
      character: "0"),
    chrome: metric(font(named: "Nunito", size: MacTheme.TypeScale.control), character: "0"),
    agent: metric(font(named: "Nunito", size: MacTheme.TypeScale.body), character: "0"),
    prose: metric(
      font(named: "Newsreader", size: PaneLayoutContentMetrics.textFontPointSize),
      character: "0"),
    screenplay: metric(
      font(named: "Courier Prime", size: PaneLayoutContentMetrics.screenplayFontPointSize),
      character: "0"))

  private static func font(named name: String, size: CGFloat) -> NSFont {
    guard let font = NSFont(name: name, size: size) else {
      assertionFailure("Missing bundled pane-layout font: \(name)")
      return NSFont.systemFont(ofSize: size)
    }
    return font
  }

  private static func metric(
    _ font: NSFont, character: String, snapToDisplayPixels: Bool = false
  ) -> PaneLayoutFontMetric {
    let coreTextFont = font as CTFont
    var characterWidth = (character as NSString).size(withAttributes: [.font: font]).width
    var lineHeight = ceil(
      CTFontGetAscent(coreTextFont) + CTFontGetDescent(coreTextFont)
        + CTFontGetLeading(coreTextFont))
    if snapToDisplayPixels {
      let scale = NSScreen.main?.backingScaleFactor ?? 2
      characterWidth = (characterWidth * scale).rounded() / scale
      lineHeight = ceil(lineHeight * scale) / scale
    }
    return PaneLayoutFontMetric(
      characterWidth: Double(characterWidth), lineHeight: Double(lineHeight),
      pointSize: Double(font.pointSize))
  }
}

enum PaneSplitAxis: Sendable {
  case columns
  case rows
}

private struct NewPaneDraft: Equatable {
  let workspaceId: String?
  var insertion: PaneLayoutProvisionalInsertion

  var placeholderPaneId: String { insertion.placeholder.paneId }

  func previewTree() throws -> PaneLayoutTree {
    try insertion.previewTree(sizing: MacPaneLayoutMetrics.sizing)
  }

  func completedTree(paneId: String, kind: PaneLayoutKind) throws -> PaneLayoutTree {
    try insertion.completedTree(
      with: PaneLayoutLeaf(paneId: paneId, kind: kind), sizing: MacPaneLayoutMetrics.sizing)
  }

  mutating func select(kind: PaneLayoutKind) {
    insertion = insertion.selecting(kind: kind)
  }
}

private struct NewPanePlaceholderPresentation {
  let draft: NewPaneDraft
  let onKindChange: (PaneLayoutKind) -> Void
  let onCancel: () -> Void
  let onCreated: (String, PaneLayoutKind) -> Void
}

struct PaneDragSource: Equatable {
  let workspaceId: String
  let paneId: String
}

struct PaneDividerGesture: Equatable {
  let workspaceId: String
  let splitPath: PaneLayoutNodePath
  let afterChildIndex: Int
  let baseTree: PaneLayoutTree
}

@Observable
final class PaneLayoutInteractionState {
  var paneSource: PaneDragSource?
  var dropTarget: PaneLayoutDropTarget?
  var divider: PaneDividerGesture?

  var isGestureActive: Bool { paneSource != nil || divider != nil }
  var blocksLayoutActions: Bool { divider != nil }

  func beginPaneDrag(workspaceId: String, paneId: String) {
    divider = nil
    dropTarget = nil
    paneSource = PaneDragSource(workspaceId: workspaceId, paneId: paneId)
  }

  func updateDropTarget(_ target: PaneLayoutDropTarget?) {
    dropTarget = target
  }

  func beginDivider(_ gesture: PaneDividerGesture) {
    paneSource = nil
    dropTarget = nil
    divider = gesture
  }

  func clearDropTarget(for paneId: String) {
    if dropTarget?.paneId == paneId { dropTarget = nil }
  }

  func clear() {
    paneSource = nil
    dropTarget = nil
    divider = nil
  }
}

struct PaneHeaderDragConfiguration {
  let interaction: PaneLayoutInteractionState
  let workspaceId: String
  let paneId: String
  let title: String
  let tint: Color
}

extension View {
  @ViewBuilder
  func paneHeaderDrag(_ configuration: PaneHeaderDragConfiguration?) -> some View {
    if let configuration {
      onDrag {
        configuration.interaction.beginPaneDrag(
          workspaceId: configuration.workspaceId, paneId: configuration.paneId)
        return NSItemProvider(
          object: "soyeht-pane:\(configuration.workspaceId):\(configuration.paneId)" as NSString)
      } preview: {
        PaneDragGhost(title: configuration.title, tint: configuration.tint)
      }
    } else {
      self
    }
  }
}

private struct PaneDragGhost: View {
  let title: String
  let tint: Color

  var body: some View {
    HStack(spacing: MacTheme.Space.x7) {
      Image(systemName: "rectangle.split.2x1")
      Text(title).lineLimit(1)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .foregroundStyle(MacTheme.ink)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.genericPaneHeaderHeight)
    .background(tint.opacity(0.92), in: Capsule())
    .shadow(color: MacTheme.ink.opacity(0.16), radius: 8, y: 4)
  }
}

struct WorkspaceWindow: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  @State private var navigation = WorkspaceNavigation()
  @State private var editorInputPreferences = EditorInputPreferences()
  @State private var webLinkPreferences = WebLinkPreferences()
  @State private var paneToClose: String?
  @State private var newPaneDraft: NewPaneDraft?
  @State private var showPairing = false
  @State private var deviceToRevoke: String?
  @State private var taskWorkspaceId: String?
  @State private var showPreferences = false
  @State private var selectedUsageProvider: UsageProvider?
  @State private var editorPaletteMode: EditorPaletteMode?
  var body: some View {
    GeometryReader { geometry in
      workspaceChrome(windowSize: geometry.size)
    }
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink).buttonStyle(.plain)
    .environment(editorInputPreferences)
    .environment(webLinkPreferences)
    .environment(\.terminalKeyboardInputEnabled, editorPaletteMode == nil)
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
    .onAppear { webHost.reconcile(client.panes) }
    .onChange(of: client.panes.map(\.paneId)) { _, _ in webHost.reconcile(client.panes) }
    .onChange(of: client.connectionRevision) { _, _ in webHost.resynchronize() }
    .onExitCommand { showPreferences = false }
  }

  private func workspaceChrome(windowSize: CGSize) -> some View {
    let policy = MacPaneLayoutMetrics.window
    let paneViewport = policy.paneViewport(
      window: PaneLayoutSize(width: windowSize.width, height: windowSize.height),
      railOpen: navigation.appBarOpen)
    let horizontalInset = policy.horizontalInset(windowWidth: windowSize.width)
    let rowWidth = max(0, windowSize.width - 2 * horizontalInset)
    let railOverlays = policy.railOverlays(windowWidth: windowSize.width)
    let contentOffset = navigation.appBarOpen && !railOverlays ? policy.railWidth : 0

    return VStack(spacing: CGFloat(policy.paneVerticalInset)) {
      WorkspaceTopBar(
        client: client, workspaceId: selectedTaskWorkspaceId,
        onSelectWorkspace: selectWorkspace, onNewPane: presentNewPane,
        onTasks: showTasks, onToggleAppBar: { navigation.toggleAppBar() })
      ZStack(alignment: .leading) {
        workspaceContent(
          viewportSize: CGSize(width: paneViewport.width, height: paneViewport.height)
        )
        .frame(width: paneViewport.width, height: paneViewport.height)
        .offset(x: contentOffset)
        if navigation.appBarOpen {
          appRail
            .frame(width: policy.railWidth, height: paneViewport.height)
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
        viewportSize: viewportSize, newPane: newPanePresentation,
        onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .terminal:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .terminal,
        viewportSize: viewportSize, onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
        onSplit: presentSplit, onTasks: showTasks)
    case .editor:
      PaneGrid(
        client: client, webHost: webHost, workspaceId: selectedTaskWorkspaceId, mode: .editor,
        viewportSize: viewportSize, onClose: { paneToClose = $0 }, onNewPane: presentNewPane,
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
      try client.paneLayouts.commit(
        tree: draft.completedTree(paneId: newPaneId, kind: kind), workspaceId: workspaceId)
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
      livePanes: client.panes.filter { $0.workspaceId == workspaceId },
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

private struct WorkspaceTopBar: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  let onSelectWorkspace: (String) -> Void
  let onNewPane: () -> Void
  let onTasks: () -> Void
  let onToggleAppBar: () -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Color.clear.frame(width: 52, height: MacPaneLayoutMetrics.topBarControlHeight)
      Button(action: onToggleAppBar) {
        Image(systemName: "rectangle.grid.2x2")
          .font(.system(size: 15, weight: .semibold))
          .frame(
            width: MacPaneLayoutMetrics.topBarControlHeight,
            height: MacPaneLayoutMetrics.topBarControlHeight
          )
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
        }
        .padding(MacPaneLayoutMetrics.workspaceTabShadowClearance)
      }
      .frame(height: MacPaneLayoutMetrics.workspaceTabViewportHeight)
      .clipped()
      .overlay {
        HStack(spacing: 0) {
          LinearGradient(
            colors: [MacTheme.canvas, MacTheme.canvas.opacity(0)],
            startPoint: .leading, endPoint: .trailing
          )
          .frame(width: MacTheme.Space.x18)
          Spacer(minLength: 0)
          LinearGradient(
            colors: [MacTheme.canvas.opacity(0), MacTheme.canvas],
            startPoint: .leading, endPoint: .trailing
          )
          .frame(width: MacTheme.Space.x18)
        }
        .frame(height: MacPaneLayoutMetrics.workspaceTabViewportHeight)
        .allowsHitTesting(false)
      }

      Button(action: onNewPane) {
        Image(systemName: "plus").font(.system(size: 14, weight: .semibold))
          .frame(
            width: MacPaneLayoutMetrics.topBarControlHeight,
            height: MacPaneLayoutMetrics.topBarControlHeight
          ).neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
      }
      .help("Open a terminal or agent pane").disabled(!client.connected)
      Spacer(minLength: MacTheme.Space.x10)
      if !client.connected {
        HStack(spacing: MacTheme.Space.x6) {
          Circle().fill(MacTheme.warning).frame(width: 7, height: 7)
          Text("Reconnecting")
            .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .semibold))
            .foregroundStyle(MacTheme.secondary)
        }
      }
    }.frame(height: MacPaneLayoutMetrics.topBarHeight)
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
      .padding(.horizontal, MacTheme.Space.x12)
      .frame(height: MacPaneLayoutMetrics.workspaceTabHeight)
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
  private static let logger = Logger(
    subsystem: "com.soyeht.next.dev.mac", category: "pane-layout")

  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String?
  let mode: PaneMode?
  let viewportSize: CGSize
  fileprivate let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onNewPane: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onTasks: () -> Void
  @State private var interaction = PaneLayoutInteractionState()

  init(
    client: EngineClient, webHost: MacWebHost, workspaceId: String?, mode: PaneMode?,
    viewportSize: CGSize, onClose: @escaping (String) -> Void,
    onNewPane: @escaping () -> Void, onSplit: @escaping (String, PaneSplitAxis) -> Void,
    onTasks: @escaping () -> Void
  ) {
    self.client = client
    self.webHost = webHost
    self.workspaceId = workspaceId
    self.mode = mode
    self.viewportSize = viewportSize
    newPane = nil
    self.onClose = onClose
    self.onNewPane = onNewPane
    self.onSplit = onSplit
    self.onTasks = onTasks
  }

  fileprivate init(
    client: EngineClient, webHost: MacWebHost, workspaceId: String?, mode: PaneMode?,
    viewportSize: CGSize, newPane: NewPanePlaceholderPresentation?,
    onClose: @escaping (String) -> Void, onNewPane: @escaping () -> Void,
    onSplit: @escaping (String, PaneSplitAxis) -> Void, onTasks: @escaping () -> Void
  ) {
    self.client = client
    self.webHost = webHost
    self.workspaceId = workspaceId
    self.mode = mode
    self.viewportSize = viewportSize
    self.newPane = newPane
    self.onClose = onClose
    self.onNewPane = onNewPane
    self.onSplit = onSplit
    self.onTasks = onTasks
  }

  private var workspacePanes: [Pane] {
    guard let workspaceId else { return [] }
    return client.panes.filter { $0.workspaceId == workspaceId }
  }

  private var panes: [Pane] {
    workspacePanes.filter { mode == nil || $0.mode == mode }
  }

  var body: some View {
    Group {
      if let layout = resolvedLayout {
        PaneLayoutTreeSurface(
          client: client, webHost: webHost, workspaceId: layout.workspaceId, panes: panes,
          tree: layout.tree, plan: layout.plan, viewportSize: viewportSize,
          interaction: interaction, interactionsEnabled: mode == nil && newPane == nil,
          newPane: newPane,
          onClose: onClose, onTasks: onTasks, onSplit: onSplit,
          onPaneDrop: commitPaneDrop
        )
        .task(id: layout.repairTaskID) {
          persistRepairIfNeeded(layout)
        }
      } else if panes.isEmpty {
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
        }
        .frame(width: viewportSize.width, height: viewportSize.height)
      }
    }
    .frame(width: viewportSize.width, height: viewportSize.height)
    .onChange(of: client.paneLayouts.failure) { _, failure in
      handleLayoutFailure(failure)
    }
    .onChange(of: activeDraftTree) { _, draft in
      if interaction.divider != nil, draft == nil { interaction.clear() }
    }
    .onChange(of: workspaceId) { previous, _ in
      if let previous { client.paneLayouts.cancelDraft(workspaceId: previous) }
      interaction.clear()
    }
    .onDisappear {
      if let workspaceId { client.paneLayouts.cancelDraft(workspaceId: workspaceId) }
      interaction.clear()
    }
    .onAppear { handleLayoutFailure(client.paneLayouts.failure) }
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

  private var resolvedLayout: ResolvedPaneGridLayout? {
    if let newPane, newPane.draft.workspaceId == workspaceId {
      do {
        let tree = try newPane.draft.previewTree()
        let sizing = MacPaneLayoutMetrics.sizing(
          newPanePlaceholderId: newPane.draft.placeholderPaneId)
        let plan = try PaneLayoutGeometry.layout(
          tree: tree,
          viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
          sizing: sizing)
        return ResolvedPaneGridLayout(
          workspaceId: workspaceId ?? "", state: PaneLayoutState(), tree: tree, plan: plan,
          insertedPaneIds: [], isProvisional: true)
      } catch {
        Self.logger.error(
          "new_pane_layout_resolution_failed error=\(error.localizedDescription)")
        return nil
      }
    }
    guard let workspaceId,
      let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return nil }
    do {
      let reconciliation = try client.paneLayouts.reconciledPresentation(
        for: workspace.workspaceId, livePanes: workspacePanes,
        sizing: MacPaneLayoutMetrics.sizing)
      let state = reconciliation.state
      guard let completeTree = state.tree else { return nil }
      let visiblePaneIds = Set(panes.map(\.paneId))
      var visibleTree: PaneLayoutTree? = completeTree
      for paneId in completeTree.paneIdsInCanonicalOrder where !visiblePaneIds.contains(paneId) {
        visibleTree = visibleTree?.removingPane(withId: paneId)
      }
      guard let tree = visibleTree else { return nil }
      let plan = try PaneLayoutGeometry.layout(
        tree: tree,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.sizing)
      return ResolvedPaneGridLayout(
        workspaceId: workspaceId, state: state, tree: tree, plan: plan,
        insertedPaneIds: reconciliation.insertedPaneIds, isProvisional: false)
    } catch {
      Self.logger.error("pane_layout_resolution_failed error=\(error.localizedDescription)")
      return nil
    }
  }

  private func persistRepairIfNeeded(_ layout: ResolvedPaneGridLayout) {
    guard !layout.isProvisional, layout.state.mode == .custom,
      !layout.insertedPaneIds.isEmpty
    else { return }
    Self.logger.notice(
      "pane_layout_reconciled_missing workspace=\(layout.workspaceId, privacy: .public) panes=\(layout.insertedPaneIds.joined(separator: ","), privacy: .public)"
    )
    do {
      guard let tree = layout.state.tree else { return }
      try client.paneLayouts.commit(tree: tree, workspaceId: layout.workspaceId)
    } catch {
      client.problem = "Couldn't repair the pane layout. No panes were removed."
    }
  }

  private var activeDraftTree: PaneLayoutTree? {
    guard let workspaceId else { return nil }
    return client.paneLayouts.presentation(for: workspaceId)?.draftTree
  }

  private func commitPaneDrop(
    sourcePaneId: String, targetPaneId: String, zone: PaneLayoutDropZone
  ) {
    guard let workspaceId, sourcePaneId != targetPaneId else {
      interaction.clear()
      return
    }
    do {
      let tree = try client.paneLayouts.beginDraft(workspaceId: workspaceId)
      let moved = try PaneLayoutTreeMutation.moving(
        tree, paneId: sourcePaneId, relativeTo: targetPaneId, zone: zone)
      try client.paneLayouts.updateDraft(workspaceId: workspaceId, tree: moved)
      try client.paneLayouts.commitDraft(workspaceId: workspaceId)
    } catch {
      client.paneLayouts.cancelDraft(workspaceId: workspaceId)
      client.problem = "Couldn't move the pane. The layout was not changed."
    }
    interaction.clear()
  }

  private func handleLayoutFailure(_ failure: PaneLayoutCommitFailure?) {
    guard let failure, failure.workspaceId == workspaceId else { return }
    client.paneLayouts.cancelDraft(workspaceId: failure.workspaceId)
    interaction.clear()
    client.problem = PaneLayoutCommitFailure.userMessage
    client.paneLayouts.clearFailure()
  }
}

private struct ResolvedPaneGridLayout {
  let workspaceId: String
  let state: PaneLayoutState
  let tree: PaneLayoutTree
  let plan: PaneLayoutGeometryResult
  let insertedPaneIds: [String]
  let isProvisional: Bool

  var repairTaskID: String {
    "\(workspaceId):\(state.mode.rawValue):\(isProvisional):\(insertedPaneIds.joined(separator: ","))"
  }
}

private struct PaneLayoutTreeSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let workspaceId: String
  let panes: [Pane]
  let tree: PaneLayoutTree
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  var body: some View {
    PaneLayoutNodeSurface(
      client: client, webHost: webHost, panesById: panesById, tintIndices: tintIndices,
      workspaceId: workspaceId, tree: tree, path: PaneLayoutNodePath(), plan: plan,
      viewportSize: viewportSize, interaction: interaction,
      interactionsEnabled: interactionsEnabled, newPane: newPane,
      onClose: onClose, onTasks: onTasks,
      onSplit: onSplit, onPaneDrop: onPaneDrop
    )
    .onDrop(
      of: [UTType.plainText],
      delegate: PaneLayoutFallbackDropDelegate(interaction: interaction))
  }

  private var panesById: [String: Pane] {
    Dictionary(uniqueKeysWithValues: panes.map { ($0.paneId, $0) })
  }

  private var tintIndices: [String: Int] {
    Dictionary(
      uniqueKeysWithValues: tree.leavesInCanonicalOrder.enumerated().map {
        ($0.element.paneId, $0.offset)
      })
  }
}

private struct PaneLayoutNodeSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panesById: [String: Pane]
  let tintIndices: [String: Int]
  let workspaceId: String
  let tree: PaneLayoutTree
  let path: PaneLayoutNodePath
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  @ViewBuilder var body: some View {
    if let geometry = plan.nodes[path] {
      switch tree {
      case .leaf(let leaf):
        if let newPane, leaf.paneId == newPane.draft.placeholderPaneId {
          placeholderSurface(newPane, geometry: geometry)
        } else if let pane = panesById[leaf.paneId] {
          leafSurface(pane: pane, geometry: geometry)
        }
      case .split(let split):
        PaneLayoutSplitSurface(
          client: client, webHost: webHost, panesById: panesById, tintIndices: tintIndices,
          workspaceId: workspaceId, split: split, path: path, plan: plan,
          viewportSize: viewportSize, interaction: interaction,
          interactionsEnabled: interactionsEnabled, newPane: newPane,
          onClose: onClose, onTasks: onTasks,
          onSplit: onSplit, onPaneDrop: onPaneDrop)
      }
    }
  }

  private func placeholderSurface(
    _ presentation: NewPanePlaceholderPresentation,
    geometry: PaneLayoutNodeGeometry
  ) -> some View {
    let halo = CGFloat(MacPaneLayoutMetrics.paneHalo)
    let width = CGFloat(geometry.frame.size.width)
    let height = CGFloat(geometry.frame.size.height)
    return NewPaneSheet(
      client: client, workspaceId: presentation.draft.workspaceId,
      mode: Binding(
        get: {
          presentation.draft.insertion.placeholder.kind == .agent ? .agent : .terminal
        },
        set: { presentation.onKindChange(PaneLayoutKind(mode: $0)) }),
      onCancel: presentation.onCancel, onCreated: presentation.onCreated
    )
    .frame(width: max(0, width - 2 * halo), height: max(0, height - 2 * halo))
    .padding(halo)
    .frame(width: width, height: height, alignment: .topLeading)
    .id(presentation.draft.placeholderPaneId)
    .accessibilityIdentifier("new-pane-placeholder")
  }

  @ViewBuilder private func leafSurface(
    pane: Pane, geometry: PaneLayoutNodeGeometry
  ) -> some View {
    let halo = CGFloat(MacPaneLayoutMetrics.paneHalo)
    let contentWidth = CGFloat(geometry.contentSize.width)
    let contentHeight = CGFloat(geometry.contentSize.height)
    if geometry.overflowAxes.isEmpty {
      paneSurface(
        pane: pane, halo: halo, contentWidth: contentWidth, contentHeight: contentHeight
      )
      .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
      .id(pane.paneId)
    } else {
      ScrollView(scrollAxes(for: geometry), showsIndicators: true) {
        paneSurface(
          pane: pane, halo: halo, contentWidth: contentWidth, contentHeight: contentHeight
        )
      }
      .scrollBounceBehavior(.basedOnSize)
      .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
      .id(pane.paneId)
    }
  }

  private func paneSurface(
    pane: Pane, halo: CGFloat, contentWidth: CGFloat, contentHeight: CGFloat
  ) -> some View {
    PaneCard(
      client: client, webHost: webHost, pane: pane,
      tint: paneTint(tintIndices[pane.paneId] ?? 0), onClose: onClose,
      onTasks: onTasks, onSplit: onSplit,
      interaction: interactionsEnabled ? interaction : nil,
      dropTargetSize: CGSize(
        width: max(0, contentWidth - 2 * halo),
        height: max(0, contentHeight - 2 * halo)),
      onPaneDrop: onPaneDrop
    )
    .frame(
      width: max(0, contentWidth - 2 * halo),
      height: max(0, contentHeight - 2 * halo)
    )
    .padding(halo)
    .frame(width: contentWidth, height: contentHeight, alignment: .topLeading)
  }

  private func scrollAxes(for geometry: PaneLayoutNodeGeometry) -> Axis.Set {
    var axes: Axis.Set = []
    if geometry.overflowAxes.contains(.horizontal) { axes.insert(.horizontal) }
    if geometry.overflowAxes.contains(.vertical) { axes.insert(.vertical) }
    return axes
  }

  private func paneTint(_ index: Int) -> Color {
    [MacTheme.agentRose, MacTheme.agentBlue, MacTheme.agentGreen, MacTheme.agentSand][index % 4]
  }
}

private struct PaneLayoutSplitSurface: View {
  @Bindable var client: EngineClient
  let webHost: MacWebHost
  let panesById: [String: Pane]
  let tintIndices: [String: Int]
  let workspaceId: String
  let split: PaneLayoutSplit
  let path: PaneLayoutNodePath
  let plan: PaneLayoutGeometryResult
  let viewportSize: CGSize
  let interaction: PaneLayoutInteractionState
  let interactionsEnabled: Bool
  let newPane: NewPanePlaceholderPresentation?
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  private var children: [RenderedPaneLayoutChild] {
    split.children.enumerated().map {
      RenderedPaneLayoutChild(index: $0.offset, branch: $0.element)
    }
  }

  var body: some View {
    if let geometry = plan.nodes[path] {
      ScrollViewReader { proxy in
        ScrollView(
          split.axis == .horizontal ? .horizontal : .vertical,
          showsIndicators: geometry.overflowAxes.contains(split.axis)
        ) {
          ZStack(alignment: .topLeading) {
            ForEach(children) { child in
              let childPath = path.appending(child.index)
              if let childGeometry = plan.nodes[childPath] {
                PaneLayoutNodeSurface(
                  client: client, webHost: webHost, panesById: panesById,
                  tintIndices: tintIndices, workspaceId: workspaceId,
                  tree: child.branch.tree, path: childPath, plan: plan,
                  viewportSize: viewportSize, interaction: interaction,
                  interactionsEnabled: interactionsEnabled, newPane: newPane,
                  onClose: onClose,
                  onTasks: onTasks, onSplit: onSplit, onPaneDrop: onPaneDrop
                )
                .frame(
                  width: childGeometry.frame.size.width,
                  height: childGeometry.frame.size.height
                )
                .offset(
                  x: childGeometry.frame.origin.x,
                  y: childGeometry.frame.origin.y
                )
                .id(child.id)
              }
            }
            if interactionsEnabled {
              ForEach(dividers) { divider in
                if let frame = dividerFrame(divider, in: geometry) {
                  PaneLayoutDividerHandle(
                    axis: divider.axis,
                    accessibilityID: divider.accessibilityID,
                    onChanged: { translation in
                      resizeDivider(divider, translation: translation)
                    },
                    onEnded: { finishDivider(divider) },
                    onDoubleClick: { equalize(divider) }
                  )
                  .frame(width: frame.width, height: frame.height)
                  .offset(x: frame.minX, y: frame.minY)
                  .zIndex(50)
                }
              }
            }
          }
          .frame(
            width: geometry.contentSize.width, height: geometry.contentSize.height,
            alignment: .topLeading)
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDisabled(!geometry.overflowAxes.contains(split.axis))
        .frame(width: geometry.frame.size.width, height: geometry.frame.size.height)
        .onAppear { scrollSelectedPane(using: proxy) }
        .onChange(of: client.selectedPaneId) { _, _ in scrollSelectedPane(using: proxy) }
      }
    }
  }

  private func scrollSelectedPane(using proxy: ScrollViewProxy) {
    guard let selectedPaneId = client.selectedPaneId,
      let childIndex = split.children.firstIndex(where: {
        $0.tree.paneIdsInCanonicalOrder.contains(selectedPaneId)
      })
    else { return }
    let identity = split.children[childIndex].tree.leavesInCanonicalOrder[0].paneId
    proxy.scrollTo(identity, anchor: .center)
  }

  private var dividers: [RenderedPaneLayoutDivider] {
    plan.dividers.compactMap { divider in
      guard divider.splitPath == path else { return nil }
      return RenderedPaneLayoutDivider(geometry: divider)
    }
  }

  private func dividerFrame(
    _ divider: RenderedPaneLayoutDivider, in geometry: PaneLayoutNodeGeometry
  ) -> CGRect? {
    guard let before = plan.nodes[path.appending(divider.afterChildIndex)] else { return nil }
    if divider.axis == .horizontal {
      return CGRect(
        x: before.frame.maxX, y: 0,
        width: MacPaneLayoutMetrics.sizing.gaps.horizontal,
        height: geometry.contentSize.height)
    }
    return CGRect(
      x: 0, y: before.frame.maxY,
      width: geometry.contentSize.width,
      height: MacPaneLayoutMetrics.sizing.gaps.vertical)
  }

  private func resizeDivider(
    _ divider: RenderedPaneLayoutDivider, translation: CGSize
  ) {
    do {
      let active: PaneDividerGesture
      if let dividerGesture = interaction.divider {
        active = dividerGesture
      } else {
        let base = try client.paneLayouts.beginDraft(workspaceId: workspaceId)
        let gesture = PaneDividerGesture(
          workspaceId: workspaceId, splitPath: divider.splitPath,
          afterChildIndex: divider.afterChildIndex, baseTree: base)
        interaction.beginDivider(gesture)
        active = gesture
      }
      guard active.workspaceId == workspaceId,
        active.splitPath == divider.splitPath,
        active.afterChildIndex == divider.afterChildIndex
      else { return }
      let delta = divider.axis == .horizontal ? translation.width : translation.height
      let tree = try PaneLayoutTreeMutation.resizing(
        active.baseTree, splitAt: active.splitPath,
        dividerIndex: active.afterChildIndex, delta: delta,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.sizing)
      try client.paneLayouts.updateDraft(workspaceId: workspaceId, tree: tree)
    } catch {
      cancelDivider(message: "Couldn't resize the panes. The layout was not changed.")
    }
  }

  private func finishDivider(_ divider: RenderedPaneLayoutDivider) {
    guard let active = interaction.divider,
      active.workspaceId == workspaceId,
      active.splitPath == divider.splitPath,
      active.afterChildIndex == divider.afterChildIndex
    else {
      interaction.clear()
      return
    }
    defer { interaction.clear() }
    do {
      guard client.paneLayouts.presentation(for: workspaceId)?.draftTree != nil else { return }
      try client.paneLayouts.commitDraft(workspaceId: workspaceId)
    } catch {
      client.paneLayouts.cancelDraft(workspaceId: workspaceId)
      client.problem = "Couldn't resize the panes. The layout was not changed."
    }
  }

  private func equalize(_ divider: RenderedPaneLayoutDivider) {
    guard !interaction.blocksLayoutActions,
      let tree = client.paneLayouts.presentation(for: workspaceId)?.displayed.tree
    else { return }
    do {
      let equalized = try PaneLayoutTreeMutation.equalizing(
        tree, splitAt: divider.splitPath,
        viewport: PaneLayoutSize(width: viewportSize.width, height: viewportSize.height),
        sizing: MacPaneLayoutMetrics.sizing)
      try client.paneLayouts.commit(tree: equalized, workspaceId: workspaceId)
    } catch {
      client.problem = "Couldn't equalize the panes. The layout was not changed."
    }
  }

  private func cancelDivider(message: String) {
    client.paneLayouts.cancelDraft(workspaceId: workspaceId)
    interaction.clear()
    client.problem = message
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

private struct RenderedPaneLayoutChild: Identifiable {
  let index: Int
  let branch: PaneLayoutBranch

  var id: String { branch.tree.leavesInCanonicalOrder[0].paneId }
}

private struct RenderedPaneLayoutDivider: Identifiable {
  let splitPath: PaneLayoutNodePath
  let afterChildIndex: Int
  let axis: PaneLayoutAxis

  init(geometry: PaneLayoutDividerGeometry) {
    splitPath = geometry.splitPath
    afterChildIndex = geometry.afterChildIndex
    axis = geometry.axis
  }

  var id: String { accessibilityID }
  var accessibilityID: String {
    let path =
      splitPath.components.isEmpty
      ? "root" : splitPath.components.map(String.init).joined(separator: "-")
    return "pane-divider-\(path)-\(afterChildIndex)"
  }
}

private struct PaneLayoutDividerHandle: View {
  let axis: PaneLayoutAxis
  let accessibilityID: String
  let onChanged: (CGSize) -> Void
  let onEnded: () -> Void
  let onDoubleClick: () -> Void
  @State private var cursorIsActive = false

  var body: some View {
    Color.clear
      .contentShape(Rectangle())
      .gesture(
        DragGesture(minimumDistance: 2)
          .onChanged { onChanged($0.translation) }
          .onEnded { _ in onEnded() }
      )
      .onTapGesture(count: 2, perform: onDoubleClick)
      .onHover { inside in
        guard inside != cursorIsActive else { return }
        cursorIsActive = inside
        if inside {
          resizeCursor.push()
        } else {
          NSCursor.pop()
        }
      }
      .onDisappear {
        if cursorIsActive {
          NSCursor.pop()
          cursorIsActive = false
        }
      }
      .accessibilityElement(children: .ignore)
      .accessibilityLabel(axis == .horizontal ? "Resize columns" : "Resize rows")
      .accessibilityIdentifier(accessibilityID)
  }

  private var resizeCursor: NSCursor {
    axis == .horizontal ? .resizeLeftRight : .resizeUpDown
  }
}

private struct PaneLayoutPaneDropDelegate: DropDelegate {
  let interaction: PaneLayoutInteractionState
  let workspaceId: String
  let targetPaneId: String
  let targetSize: CGSize
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  func validateDrop(info: DropInfo) -> Bool {
    guard let source = interaction.paneSource else { return false }
    return source.workspaceId == workspaceId && source.paneId != targetPaneId
  }

  func dropEntered(info: DropInfo) {
    updateTarget(info.location)
  }

  func dropUpdated(info: DropInfo) -> DropProposal? {
    updateTarget(info.location)
    return interaction.dropTarget == nil
      ? DropProposal(operation: .cancel)
      : DropProposal(
        operation: .move)
  }

  func dropExited(info: DropInfo) {
    interaction.clearDropTarget(for: targetPaneId)
  }

  func performDrop(info: DropInfo) -> Bool {
    updateTarget(info.location)
    guard let source = interaction.paneSource,
      let target = interaction.dropTarget,
      target.paneId == targetPaneId,
      target.workspaceId == workspaceId
    else {
      interaction.clear()
      return false
    }
    onPaneDrop(source.paneId, target.paneId, target.zone)
    interaction.clear()
    return true
  }

  private func updateTarget(_ location: CGPoint) {
    guard let source = interaction.paneSource else {
      interaction.updateDropTarget(nil)
      return
    }
    let rect = PaneLayoutRect(
      size: PaneLayoutSize(width: targetSize.width, height: targetSize.height))
    interaction.updateDropTarget(
      PaneLayoutDropGeometry.target(
        at: PaneLayoutPoint(x: location.x, y: location.y),
        sourceWorkspaceId: source.workspaceId, targetWorkspaceId: workspaceId,
        targetPaneId: targetPaneId, paneArea: rect, targetPaneRect: rect))
  }
}

private struct PaneLayoutFallbackDropDelegate: DropDelegate {
  let interaction: PaneLayoutInteractionState

  func validateDrop(info: DropInfo) -> Bool {
    interaction.paneSource != nil
  }

  func performDrop(info: DropInfo) -> Bool {
    interaction.clear()
    return false
  }
}

private struct PaneDropTargetModifier: ViewModifier {
  let interaction: PaneLayoutInteractionState?
  let workspaceId: String
  let paneId: String
  let size: CGSize?
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void

  @ViewBuilder
  func body(content: Content) -> some View {
    if let interaction, let size {
      content.onDrop(
        of: [UTType.plainText],
        delegate: PaneLayoutPaneDropDelegate(
          interaction: interaction, workspaceId: workspaceId, targetPaneId: paneId,
          targetSize: size, onPaneDrop: onPaneDrop))
    } else {
      content
    }
  }
}

private struct PaneDropZonesOverlay: View {
  let paneId: String
  let size: CGSize
  let selected: PaneLayoutDropZone?

  var body: some View {
    ZStack(alignment: .topLeading) {
      ForEach(PaneLayoutDropZone.allCases, id: \.rawValue) { zone in
        let rect = PaneLayoutDropGeometry.rect(
          for: zone,
          in: PaneLayoutRect(size: PaneLayoutSize(width: size.width, height: size.height)))
        RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
          .fill(selected == zone ? MacTheme.accent.opacity(0.24) : Color.clear)
          .overlay {
            if selected == zone {
              RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
                .stroke(MacTheme.accent, lineWidth: 2)
            }
          }
          .frame(width: rect.size.width, height: rect.size.height)
          .offset(x: rect.origin.x, y: rect.origin.y)
          .accessibilityElement(children: .ignore)
          .accessibilityLabel("Drop pane \(zone.rawValue)")
          .accessibilityIdentifier("pane-drop-zone-\(zone.rawValue)-\(paneId)")
      }
    }
    .frame(width: size.width, height: size.height, alignment: .topLeading)
    .allowsHitTesting(false)
  }
}

private struct PaneCard: View {
  @Bindable var client: EngineClient
  @Environment(\.terminalKeyboardInputEnabled) private var terminalKeyboardInputEnabled
  let webHost: MacWebHost
  let pane: Pane
  let tint: Color
  let onClose: (String) -> Void
  let onTasks: () -> Void
  let onSplit: (String, PaneSplitAxis) -> Void
  let interaction: PaneLayoutInteractionState?
  let dropTargetSize: CGSize?
  let onPaneDrop: (String, String, PaneLayoutDropZone) -> Void
  @State private var showingAutomation = false
  @State private var showingHarnessSwitch = false
  @State private var showingCaptureConsent = false
  @State private var showingCaptureDeletion = false
  @State private var captureMutationInFlight = false
  @State private var captureNotice: String?

  var body: some View {
    Group {
      if pane.mode == .editor {
        EditorPaneView(client: client, pane: pane, headerDrag: dragConfiguration)
      } else if pane.mode == .writing, pane.writingKind == .text {
        WritingTextPane(client: client, pane: pane, headerDrag: dragConfiguration)
      } else if pane.mode == .writing, pane.writingKind == .presentation {
        WritingPresentationPane(client: client, pane: pane, headerDrag: dragConfiguration)
      } else if pane.mode == .writing, pane.writingKind == .screenplay {
        WritingScreenplayPane(client: client, pane: pane, headerDrag: dragConfiguration)
      } else {
        VStack(spacing: PaneLayoutContentMetrics.paneStackSpacing) {
          header
          paneBody
        }
        .padding(PaneLayoutContentMetrics.paneCardInset)
        .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
      }
    }
    .contentShape(RoundedRectangle(cornerRadius: MacTheme.Radius.card))
    .simultaneousGesture(TapGesture().onEnded { client.select(pane.paneId) })
    .modifier(
      PaneDropTargetModifier(
        interaction: interaction, workspaceId: pane.workspaceId, paneId: pane.paneId,
        size: dropTargetSize, onPaneDrop: onPaneDrop)
    )
    .overlay(alignment: .topLeading) {
      if let interaction, let dropTargetSize {
        PaneDropZonesOverlay(
          paneId: pane.paneId, size: dropTargetSize,
          selected: interaction.dropTarget?.paneId == pane.paneId
            ? interaction.dropTarget?.zone : nil)
      }
    }
    .contextMenu { Button("Close pane", role: .destructive) { onClose(pane.paneId) } }
    .confirmationDialog(
      ExperimentCapturePresentation.consentTitle(for: pane.mode),
      isPresented: $showingCaptureConsent
    ) {
      Button("Start recording") { setExperimentCapture(on: true) }
      Button("Cancel", role: .cancel) {}
    } message: {
      Text(ExperimentCapturePresentation.consentMessage(for: pane.mode))
    }
    .sheet(isPresented: $showingCaptureDeletion) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x18) {
        Text("Keep the captured experiment data?")
          .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .semibold))
        Text(
          "Recording has stopped. Delete removes only this pane's recorded segments. Export files remain at \(experimentExportsPath)."
        )
        .font(MacTheme.chrome(MacTheme.TypeScale.control))
        .foregroundStyle(MacTheme.secondary)
        HStack {
          Button("Keep data") {
            captureNotice = "Captured data kept on this Mac."
            showingCaptureDeletion = false
          }
          Spacer()
          Button("Delete captured data", role: .destructive) {
            showingCaptureDeletion = false
            deleteExperimentCaptureData()
          }
          .keyboardShortcut(.defaultAction)
        }
      }
      .padding(MacTheme.Space.x18)
      .frame(width: 420)
      .interactiveDismissDisabled()
    }
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

  private var dragConfiguration: PaneHeaderDragConfiguration? {
    guard let interaction else { return nil }
    return PaneHeaderDragConfiguration(
      interaction: interaction, workspaceId: pane.workspaceId, paneId: pane.paneId,
      title: pane.name, tint: tint)
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
      #if DEBUG
        if [.terminal, .agent].contains(pane.mode), let capture = pane.experimentCapture {
          if capture.on {
            Label(
              ExperimentCapturePresentation.activeLabel(for: pane.mode),
              systemImage: "record.circle.fill"
            )
            .foregroundStyle(MacTheme.danger)
            .accessibilityIdentifier("experiment-recording-\(pane.paneId)")
          } else if let failure = capture.failure {
            Label("Capture stopped", systemImage: "exclamationmark.triangle.fill")
              .foregroundStyle(MacTheme.warning)
              .help(failure)
              .accessibilityLabel("Capture stopped: \(failure)")
          } else if let captureNotice {
            Text(captureNotice).foregroundStyle(MacTheme.secondary)
          }
        }
        if [.terminal, .agent].contains(pane.mode) {
          Button {
            if pane.experimentCapture?.on == true {
              setExperimentCapture(on: false)
            } else {
              showingCaptureConsent = true
            }
          } label: {
            Image(systemName: pane.experimentCapture?.on == true ? "stop.circle" : "record.circle")
          }
          .help(
            pane.experimentCapture?.on == true
              ? "Stop experiment recording" : "Start experiment recording"
          )
          .accessibilityLabel(
            pane.experimentCapture?.on == true
              ? "Stop experiment recording" : "Start experiment recording"
          )
          .accessibilityIdentifier("experiment-capture-\(pane.paneId)")
          .disabled(!client.connected || captureMutationInFlight)
        }
      #endif
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
      .disabled(!client.connected || interaction?.blocksLayoutActions == true)
      Button {
        onSplit(pane.paneId, .rows)
      } label: {
        Image(systemName: "rectangle.split.1x2")
      }
      .help("Split pane into rows")
      .accessibilityLabel("Split pane into rows")
      .accessibilityIdentifier("split-pane-rows-\(pane.paneId)")
      .disabled(!client.connected || interaction?.blocksLayoutActions == true)
      Button {
        onClose(pane.paneId)
      } label: {
        Image(systemName: "xmark")
      }
      .help("Close this pane")
      .disabled(!client.connected || interaction?.blocksLayoutActions == true)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.genericPaneHeaderHeight)
    .background(tint, in: Capsule())
    .paneHeaderDrag(dragConfiguration)
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
          .padding(.horizontal, MacTheme.Space.x12)
          .frame(height: PaneLayoutContentMetrics.agentTaskHeight)
          .neoRaised(radius: MacTheme.Radius.control, elevation: .chip)
        }
        AgentConversationView(
          client: client, paneId: pane.paneId, showsConfiguration: false,
          style: MacTheme.agentConversation)
      }.padding(
        EdgeInsets(
          top: PaneLayoutContentMetrics.agentBodyTopInset, leading: 4, bottom: 0,
          trailing: 4))
    } else {
      VStack(spacing: 0) {
        Label(
          TerminalStatePresentation.text(for: pane.state),
          systemImage: terminalStateSystemImage
        )
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .foregroundStyle(stateColor)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, MacTheme.Space.x10)
        .padding(.vertical, PaneLayoutContentMetrics.terminalStateVerticalInset)
        .background(tint.opacity(0.55))
        .accessibilityIdentifier("terminal-state-\(pane.paneId)")
        MacTerminal(client: client, paneId: pane.paneId)
          .environment(
            \.terminalKeyboardInputEnabled,
            terminalKeyboardInputEnabled && client.selectedPaneId == pane.paneId)
        if let gap = client.gapText(for: pane.paneId) {
          Label(gap, systemImage: "exclamationmark.triangle")
            .font(MacTheme.chrome(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.warning)
            .padding(MacTheme.Space.x10)
        }
      }
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

  private var terminalStateSystemImage: String {
    switch pane.state {
    case .working: "bolt.fill"
    case .idle: "checkmark.circle.fill"
    case .waiting_for_you: "person.crop.circle.badge.questionmark"
    case .no_signal: "exclamationmark.triangle.fill"
    case .dead: "xmark.circle.fill"
    case .starting, .recovering: "arrow.triangle.2.circlepath"
    case .cancelling: "stop.circle.fill"
    case .unknown: "questionmark.circle.fill"
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

  private func setExperimentCapture(on: Bool) {
    captureMutationInFlight = true
    Task {
      defer { captureMutationInFlight = false }
      do {
        let result = try await client.setExperimentCapture(paneId: pane.paneId, on: on)
        if result.capture.on {
          captureNotice = nil
        } else {
          captureNotice = "Recording stopped."
          showingCaptureDeletion = true
        }
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private var experimentExportsPath: String {
    DevProfile.dataDirectory
      .appendingPathComponent("experiments/pane-state/exports", isDirectory: true).path
  }

  private func deleteExperimentCaptureData() {
    captureMutationInFlight = true
    Task {
      defer { captureMutationInFlight = false }
      do {
        let result = try await client.deleteExperimentCaptureData(paneId: pane.paneId)
        captureNotice = ExperimentCapturePresentation.deletionText(result)
      } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct NewPaneSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  @Binding var mode: PaneMode
  let onCancel: () -> Void
  let onCreated: (String, PaneLayoutKind) -> Void
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
        Button(action: onCancel) {
          Image(systemName: "xmark")
        }
        .help("Close new pane")
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.secondary)
      .padding(.horizontal, MacTheme.Space.x12)
      .frame(height: PaneLayoutContentMetrics.newPaneHeaderHeight)
      .background(MacTheme.well, in: Capsule())

      ScrollView(.vertical, showsIndicators: true) {
        VStack(spacing: PaneLayoutContentMetrics.newPaneSectionSpacing) {
          Text("What do you want to do?")
            .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
            .frame(height: PaneLayoutContentMetrics.newPaneHeadingHeight)
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
          .frame(maxWidth: .infinity)
          .frame(height: PaneLayoutContentMetrics.newPaneFormHeight)
          .neoWell(radius: MacTheme.Radius.control)

          metadata
            .frame(height: PaneLayoutContentMetrics.newPaneMetadataHeight)
          ViewThatFits(in: .horizontal) {
            modeSwitch(horizontal: true)
            modeSwitch(horizontal: false)
          }
          .frame(
            maxWidth: .infinity,
            minHeight: PaneLayoutContentMetrics.newPaneCompactFooterHeight,
            alignment: .topLeading)
        }
        .padding(.vertical, PaneLayoutContentMetrics.newPaneBodyVerticalInset)
        .frame(maxWidth: .infinity)
      }
      .scrollBounceBehavior(.basedOnSize)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
    .padding(PaneLayoutContentMetrics.paneCardInset)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
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

  @ViewBuilder private func modeSwitch(horizontal: Bool) -> some View {
    let content = Group {
      Text(mode == .agent ? "Prefer the command line?" : "Prefer a structured agent?")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
      Button(action: toggleMode) {
        Label(
          mode == .agent ? "Open a terminal" : "Open an agent",
          systemImage: mode == .agent ? "terminal" : "cpu")
      }
      .buttonStyle(NeoPillButtonStyle())
      Text("⌘⇧Enter")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
    }
    if horizontal {
      HStack(spacing: MacTheme.Space.x10) { content }
    } else {
      VStack(alignment: .leading, spacing: MacTheme.Space.x8) { content }
    }
  }

  private func toggleMode() {
    mode = mode == .agent ? .terminal : .agent
    if mode == .agent {
      showsAgentPicker = true
      promptFocused = true
    }
  }

  private var metadata: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Button(action: chooseDirectory) {
        Label(
          directory?.lastPathComponent ?? "Project", systemImage: "folder"
        )
        .lineLimit(1)
      }
      .layoutPriority(1)
      Label("Local", systemImage: "desktopcomputer")
        .fixedSize()
      Label("main", systemImage: "arrow.triangle.branch")
        .fixedSize()
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
    .foregroundStyle(MacTheme.secondary)
    .frame(maxWidth: .infinity)
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
        let paneId: String
        if mode == .terminal {
          paneId = try await client.createTerminalPane(directory: directory)
        } else {
          paneId = try await client.createAgentPaneNow(
            directory: directory, harness: harness, model: model.isEmpty ? nil : model,
            effort: effort.isEmpty ? nil : effort, permission: permission)
        }
        onCreated(paneId, PaneLayoutKind(mode: mode))
        if mode == .agent {
          if !firstPrompt.isEmpty {
            try await client.sendPromptNow(paneId, text: firstPrompt)
          }
        }
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

  private var contentState: HarnessPickerContentState {
    HarnessPickerModel.contentState(
      harnesses: client.harnesses.filter { $0.id != excludingHarness }, query: query,
      loading: client.harnessesLoading)
  }

  private var modelContentState: ModelPickerContentState {
    HarnessPickerModel.modelContentState(
      catalogue: catalogue, loading: client.agentCatalogueLoading(harness))
  }

  private var selected: HarnessInfo? { client.harnesses.first { $0.id == harness } }
  private var selectedModel: ModelInfo? { catalogue?.models.first { $0.id == model } }
  private var excludedQueryMatch: HarnessInfo? {
    guard !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
      let excludingHarness,
      let excluded = client.harnesses.first(where: { $0.id == excludingHarness }),
      HarnessPickerModel.matches(excluded, query: query)
    else { return nil }
    return excluded
  }

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
      if contentState == .loading {
        VStack(spacing: MacTheme.Space.x10) {
          ProgressView()
          Text("Loading agents…")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
        .frame(maxWidth: .infinity, minHeight: 220)
        .accessibilityIdentifier("agent-picker-loading")
      } else if contentState == .emptyCatalogue {
        ContentUnavailableView(
          "No agents available", systemImage: "cpu",
          description: Text("Rescan after installing or configuring an agent.")
        )
        .frame(maxWidth: .infinity, minHeight: 220)
      } else if contentState == .noMatches {
        if let excludedQueryMatch {
          ContentUnavailableView(
            "\(excludedQueryMatch.name) is already active", systemImage: "checkmark.circle",
            description: Text("Choose another agent to restart this conversation.")
          )
          .frame(maxWidth: .infinity, minHeight: 220)
        } else {
          ContentUnavailableView.search(text: query)
            .frame(maxWidth: .infinity, minHeight: 220)
        }
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
    .onAppear {
      if selected?.available == true, !harness.isEmpty {
        client.refreshCatalogue(harness: harness)
      }
    }
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
      if modelContentState == .loading {
        HStack(spacing: MacTheme.Space.x8) {
          ProgressView().controlSize(.small)
          Text("Loading models…")
        }
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
        .padding(.horizontal, MacTheme.Space.x10).frame(height: 30)
        .neoWell(radius: MacTheme.Radius.compact)
        .accessibilityIdentifier("model-picker-loading")
      } else {
        Menu {
          Button("Agent default") { model = "" }
          ForEach(catalogue?.models ?? [], id: \.id) { item in
            Button {
              model = item.id
            } label: {
              if item.observedRefusal == nil {
                Text(item.name)
              } else {
                Label(
                  HarnessPickerModel.modelLabel(for: item),
                  systemImage: "exclamationmark.triangle.fill")
              }
            }
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
        if let reason = HarnessPickerModel.modelRefusalReason(for: selectedModel) {
          Label {
            Text("This agent previously refused this model: \(reason)")
          } icon: {
            Image(systemName: "exclamationmark.triangle.fill")
          }
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.warning)
          .fixedSize(horizontal: false, vertical: true)
          .accessibilityIdentifier("model-observed-refusal")
        }
        if modelContentState == .empty {
          Text("This agent provided no model choices.")
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        } else if modelContentState == .unavailable {
          Text("Models will appear after this agent starts.")
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
      }
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
    selectedModel.map { HarnessPickerModel.modelLabel(for: $0) } ?? "Agent default"
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
