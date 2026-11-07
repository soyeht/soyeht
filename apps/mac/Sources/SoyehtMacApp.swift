import AppKit
import Observation
import SoyehtClient
import SoyehtDictation
import SoyehtProtocol
import SwiftUI
import UserNotifications
import os

@main @MainActor
struct SoyehtMacApp: App {
  @State private var client: EngineClient
  @State private var themeStore = SoyehtThemeStore.shared
  @State private var attentionNotifications = AgentAttentionNotifications()
  @State private var editorInputPreferences = EditorInputPreferences()
  @State private var webLinkPreferences = WebLinkPreferences()
  @State private var engineService = EngineServiceSetup()
  @State private var appRelocation = AppRelocation()
  @State private var updateChecker = UpdateChecker()
  @State private var diagnosticsCopy = DiagnosticsCopy()
  @State private var crashNotice = CrashNotice()
  private let webHost: MacWebHost

  init() {
    let host = MacWebHost()
    let client = EngineClient(webHost: host)
    host.bind(client)
    webHost = host
    _client = State(initialValue: client)
    MacTheme.registerFonts()
    DictationEngineStore.shared.install(DictationCatalogue.registry)
  }

  var body: some Scene {
    Window(ActiveProfile.windowTitle, id: "main") {
      #if SOYEHT_PERFORMANCE_METER
        if let options = EditorPerformanceFixture.requestedOptions() {
          Color.clear.onAppear {
            NSApp.mainWindow?.orderOut(nil)
            EditorPerformanceFixture.launch(options)
          }
        } else {
          workspaceContent
        }
      #else
        workspaceContent
      #endif
    }
    .defaultSize(width: 1440, height: 900)
    .windowStyle(.hiddenTitleBar)
    .commands {
      CommandGroup(replacing: .newItem) {}
      CommandGroup(after: .appInfo) {
        if appRelocation.canMove {
          Button(ClientPolicy.appMoveMenu) { Task { await appRelocation.move() } }
        }
      }
      CommandGroup(after: .toolbar) {
        Button("Command Palette") {
          NotificationCenter.default.post(
            name: .showSoyehtEditorPalette, object: EditorPaletteMode.commands)
        }
        .keyboardShortcut("p", modifiers: [.command, .shift])
        Button("Open File") {
          NotificationCenter.default.post(
            name: .showSoyehtEditorPalette, object: EditorPaletteMode.files)
        }
        .keyboardShortcut("p", modifiers: .command)
        Button("Search Project") {
          NotificationCenter.default.post(
            name: .showSoyehtEditorPalette, object: EditorPaletteMode.project)
        }
        .keyboardShortcut("f", modifiers: [.command, .shift])
        Button("Go to Line") {
          NotificationCenter.default.post(
            name: .showSoyehtEditorPalette, object: EditorPaletteMode.line)
        }
        .keyboardShortcut("g", modifiers: .control)
        Divider()
        Button("Toggle App Bar") {
          NotificationCenter.default.post(name: .toggleSoyehtAppBar, object: nil)
        }
        .keyboardShortcut("b", modifiers: [.command, .shift])
        // Zed's project panel key. The visible editor toggles its workspace's panel.
        Button("Toggle Project Panel") {
          NotificationCenter.default.post(name: .toggleSoyehtProjectPanel, object: nil)
        }
        .keyboardShortcut(Self.keyboardShortcut(.projectPanel))
        // Cmd+Option+Z: a key equivalent, never text (see `EditorShortcut.softWrap`).
        Toggle(
          "Soft Wrap",
          isOn: Binding(
            get: { editorInputPreferences.softWrap },
            set: { editorInputPreferences.setSoftWrap($0) })
        )
        .keyboardShortcut(Self.keyboardShortcut(.softWrap))
        Button("Switch Dictation Engine", action: Self.switchDictationEngine)
          .keyboardShortcut(DictationShortcut.key, modifiers: DictationShortcut.modifiers)
        Divider()
        workspaceViewCommand("Agents", destination: .agents, key: "1")
        workspaceViewCommand("Editor", destination: .editor, key: "2")
        workspaceViewCommand("Web", destination: .web, key: "3")
        workspaceViewCommand("Tasks", destination: .tasks, key: "4")
        workspaceViewCommand("Terminal", destination: .terminal, key: "5")
        workspaceViewCommand("iPhone", destination: .phone, key: "6")
        workspaceViewCommand("Texts", destination: .texts, key: "7")
        workspaceViewCommand("Presentations", destination: .presentations, key: "8")
        workspaceViewCommand("Screenplays", destination: .screenplays, key: "9")
      }
      CommandGroup(replacing: .appSettings) {
        Button("Preferences…") {
          NotificationCenter.default.post(name: .showSoyehtPreferences, object: nil)
        }
        .keyboardShortcut(",", modifiers: .command)
      }
    }
  }

  private var workspaceContent: some View {
    WorkspaceWindow(client: client, webHost: webHost)
      .ignoresSafeArea(.container, edges: .top)
      .frame(
        minWidth: MacPaneLayoutMetrics.minimumWindow.width,
        minHeight: MacPaneLayoutMetrics.minimumWindow.height
      )
      .environment(themeStore)
      .environment(attentionNotifications)
      .environment(editorInputPreferences)
      .environment(webLinkPreferences)
      .environment(engineService)
      .environment(appRelocation)
      .environment(updateChecker)
      .environment(diagnosticsCopy)
      .environment(crashNotice)
      .preferredColorScheme(themeStore.activeTheme.palette.isDark ? .dark : .light)
      .task { client.start() }
      .task { updateChecker.start() }
      .task { await engineService.refresh() }
      .task { await appRelocation.removeLeftovers() }
      .task { await crashNotice.start() }
      .alert(
        ClientPolicy.loginStartQuestion,
        isPresented: Binding(
          get: { engineService.shouldAsk && !client.connected },
          set: { _ in })
      ) {
        Button(ClientPolicy.loginStartTurnOn) { Task { await engineService.enable() } }
        Button(ClientPolicy.loginStartNotNow, role: .cancel) { engineService.decline() }
      } message: {
        Text(verbatim: ClientPolicy.loginStartSummary + " " + ClientPolicy.loginStartLater)
      }
      .modifier(AppRelocationAlerts(relocation: appRelocation))
  }

  /// Cycles the installed engines. With only one installed, the install step of another opens
  /// in Preferences; nothing downloads.
  private static func switchDictationEngine() {
    if DictationEngineStore.shared.cycle() == .needsInstall {
      NotificationCenter.default.post(name: .showSoyehtPreferences, object: nil)
    }
  }

  private static func keyboardShortcut(_ shortcut: EditorShortcut) -> KeyboardShortcut {
    var modifiers: EventModifiers = .command
    if shortcut.shift { modifiers.insert(.shift) }
    if shortcut.option { modifiers.insert(.option) }
    if shortcut.control { modifiers.insert(.control) }
    return KeyboardShortcut(KeyEquivalent(Character(shortcut.key)), modifiers: modifiers)
  }

  private func workspaceViewCommand(
    _ title: String, destination: WorkspaceDestination, key: KeyEquivalent
  ) -> some View {
    Button(title) {
      NotificationCenter.default.post(name: .selectSoyehtWorkspaceView, object: destination)
    }
    .keyboardShortcut(key, modifiers: .command)
  }
}

/// The questions of a released app that runs outside Applications (ADR 0120): the one offer, the
/// confirmation before a copy there is replaced, and the message when a move did not end in a new
/// copy opening. Each alert is shown for as long as the state asks it; the buttons change the state.
private struct AppRelocationAlerts: ViewModifier {
  let relocation: AppRelocation

  func body(content: Content) -> some View {
    content
      .alert(ClientPolicy.appMoveQuestion, isPresented: shown(\.asksToMove)) {
        Button(ClientPolicy.appMoveAction) { Task { await relocation.move() } }
        Button(ClientPolicy.loginStartNotNow, role: .cancel) { relocation.decline() }
      } message: {
        Text(verbatim: ClientPolicy.appMoveSummary)
      }
      .alert(ClientPolicy.appMoveReplaceQuestion, isPresented: shown(\.asksToReplace)) {
        Button(ClientPolicy.appMoveReplace, role: .destructive) {
          Task { await relocation.confirmReplace() }
        }
        Button(ClientPolicy.cancel, role: .cancel) { relocation.cancelReplace() }
      } message: {
        Text(verbatim: ClientPolicy.appMoveReplaceSummary)
      }
      .alert(relocation.reportTitle, isPresented: shown(\.hasReport)) {
        if relocation.offersRetry {
          Button(ClientPolicy.tryAgain) {
            relocation.acknowledge()
            Task { await relocation.move() }
          }
        }
        Button(ClientPolicy.ok, role: .cancel) { relocation.acknowledge() }
      } message: {
        Text(verbatim: messageText)
      }
  }

  private var messageText: String {
    guard let report = relocation.report else { return "" }
    return relocation.offersRetry ? report + " " + ClientPolicy.appMoveFailedLater : report
  }

  /// The state decides when an alert shows; a dismissal changes it through a button.
  private func shown(_ asks: KeyPath<AppRelocation, Bool>) -> Binding<Bool> {
    Binding(get: { relocation[keyPath: asks] }, set: { _ in })
  }
}

struct AgentAttentionRoute: Equatable, Sendable {
  let workspaceId: String
  let paneId: String
  let requestIdentity: String
}

private final class AgentAttentionNotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
  var onOpen: (@MainActor @Sendable (AgentAttentionRoute) -> Void)?

  func userNotificationCenter(
    _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
    withCompletionHandler completionHandler: @escaping () -> Void
  ) {
    let info = response.notification.request.content.userInfo
    let route: AgentAttentionRoute? = {
      guard let workspaceId = info["workspaceId"] as? String,
        let paneId = info["paneId"] as? String,
        let requestIdentity = info["requestIdentity"] as? String
      else { return nil }
      return AgentAttentionRoute(
        workspaceId: workspaceId, paneId: paneId, requestIdentity: requestIdentity)
    }()
    if let route {
      let onOpen = onOpen
      Task { @MainActor in
        onOpen?(route)
      }
    }
    completionHandler()
  }
}

@MainActor @Observable
final class AgentAttentionNotifications {
  private static let logger = Logger(
    subsystem: "com.soyeht.next", category: "agent-attention-notifications")
  private let center = UNUserNotificationCenter.current()
  private let delegate = AgentAttentionNotificationDelegate()
  private let settings = AgentAttentionNotificationSettings()
  private var cursor = AgentAttentionNotificationCursor()
  var enabled: Bool { settings.enabled }
  private(set) var pendingRoute: AgentAttentionRoute?

  init() {
    delegate.onOpen = { [weak self] route in self?.pendingRoute = route }
    center.delegate = delegate
    // Nothing is asked here. macOS is asked when the first notification is about to be
    // delivered (`observe`) or when the person turns notifications on (`setEnabled`).
  }

  func clearPendingRoute() { pendingRoute = nil }

  private func requestAuthorization() async -> Bool {
    do {
      return try await center.requestAuthorization(options: [.alert, .badge])
    } catch {
      Self.logger.error("Notification authorization failed")
      return false
    }
  }

  func setEnabled(_ requested: Bool) async {
    settings.setEnabled(requested ? await requestAuthorization() : false)
  }

  func observe(_ summary: AgentAttentionSummary, workspaces: [Workspace], connected: Bool) {
    NSApp.dockTile.badgeLabel =
      summary.totalWaitingCount == 0
      ? nil : String(summary.totalWaitingCount)
    NSApp.dockTile.display()
    let newRequests = cursor.newRequests(in: summary, connected: connected)
    guard settings.shouldPost(appIsActive: NSApp.isActive), !newRequests.isEmpty else { return }
    var notifications: [UNNotificationRequest] = []
    for request in newRequests {
      let content = UNMutableNotificationContent()
      content.title = AgentAttentionPolicy.notificationTitle
      content.body = AgentAttentionPolicy.notificationBody(
        paneName: request.paneName,
        workspaceName: workspaces.first { $0.workspaceId == request.workspaceId }?.name)
      content.userInfo = [
        "workspaceId": request.workspaceId,
        "paneId": request.paneId,
        "requestIdentity": request.identity,
      ]
      notifications.append(
        UNNotificationRequest(identifier: request.identity, content: content, trigger: nil))
    }
    Task { await deliver(notifications) }
  }

  private func permission() async -> AgentAttentionNotificationSettings.Permission {
    switch await center.notificationSettings().authorizationStatus {
    case .notDetermined: .notDetermined
    case .denied: .denied
    default: .granted
    }
  }

  private func deliver(_ notifications: [UNNotificationRequest]) async {
    switch settings.delivery(appIsActive: NSApp.isActive, permission: await permission()) {
    case .skip: return
    case .askThenPost:
      // The answer is the system's; a refusal turns the setting off, as the toggle does.
      let granted = await requestAuthorization()
      settings.setEnabled(granted)
      if !granted { return }
    case .post: break
    }
    for notification in notifications {
      do { try await center.add(notification) } catch {
        Self.logger.error("Could not post agent-attention notification")
      }
    }
  }
}

extension Notification.Name {
  static let showSoyehtPreferences = Notification.Name("showSoyehtPreferences")
  static let toggleSoyehtAppBar = Notification.Name("toggleSoyehtAppBar")
  static let selectSoyehtWorkspaceView = Notification.Name("selectSoyehtWorkspaceView")
  static let showSoyehtEditorPalette = Notification.Name("showSoyehtEditorPalette")
  static let toggleSoyehtProjectPanel = Notification.Name("toggleSoyehtProjectPanel")
}
