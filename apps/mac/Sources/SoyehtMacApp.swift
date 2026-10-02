import AppKit
import Observation
import SoyehtClient
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
  private let webHost: MacWebHost

  init() {
    let host = MacWebHost()
    let client = EngineClient(webHost: host)
    host.bind(client)
    webHost = host
    _client = State(initialValue: client)
    MacTheme.registerFonts()
  }

  var body: some Scene {
    Window("Soyeht · Dev", id: "main") {
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
        minWidth: MacPaneLayoutMetrics.minimumExercisedViewport.width,
        minHeight: MacPaneLayoutMetrics.minimumExercisedViewport.height
      )
      .environment(themeStore)
      .environment(attentionNotifications)
      .environment(editorInputPreferences)
      .preferredColorScheme(themeStore.activeTheme.palette.isDark ? .dark : .light)
      .task { client.start() }
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
  }

  func clearPendingRoute() { pendingRoute = nil }

  func setEnabled(_ requested: Bool) async {
    if requested {
      do {
        settings.setEnabled(try await center.requestAuthorization(options: [.alert, .badge]))
      } catch {
        settings.setEnabled(false)
        Self.logger.error("Notification authorization failed")
      }
    } else {
      settings.setEnabled(false)
    }
  }

  func observe(_ summary: AgentAttentionSummary, workspaces: [Workspace], connected: Bool) {
    NSApp.dockTile.badgeLabel =
      summary.totalWaitingCount == 0
      ? nil : String(summary.totalWaitingCount)
    NSApp.dockTile.display()
    let newRequests = cursor.newRequests(in: summary, connected: connected)
    guard settings.shouldPost(appIsActive: NSApp.isActive) else { return }
    for request in newRequests {
      let workspaceName =
        workspaces.first { $0.workspaceId == request.workspaceId }?.name
        ?? "Workspace"
      let content = UNMutableNotificationContent()
      content.title = "Agent needs your input"
      content.body = "\(request.paneName) · \(workspaceName)"
      content.userInfo = [
        "workspaceId": request.workspaceId,
        "paneId": request.paneId,
        "requestIdentity": request.identity,
      ]
      let notification = UNNotificationRequest(
        identifier: request.identity, content: content, trigger: nil)
      Task {
        do { try await center.add(notification) } catch {
          Self.logger.error("Could not post agent-attention notification")
        }
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
