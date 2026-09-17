import SoyehtClient
import SwiftUI

@main @MainActor
struct SoyehtMacApp: App {
  @State private var client: EngineClient
  @State private var themeStore = SoyehtThemeStore.shared
  private let webHost: MacWebHost

  init() {
    let host = MacWebHost()
    let client = EngineClient(webHost: host)
    host.bind(client)
    webHost = host
    _client = State(initialValue: client)
    MacTheme.registerFonts()
    #if DEBUG
      if let report = EditorPerformanceProbe.requestedReportURL() {
        EditorPerformanceProbe.runAndExit(report: report)
      }
    #endif
  }

  var body: some Scene {
    Window("Soyeht · Dev", id: "main") {
      WorkspaceWindow(client: client, webHost: webHost)
        .ignoresSafeArea(.container, edges: .top)
        .frame(minWidth: 1024, minHeight: 640)
        .environment(themeStore)
        .preferredColorScheme(themeStore.activeTheme.palette.isDark ? .dark : .light)
        .task { client.start() }
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

  private func workspaceViewCommand(
    _ title: String, destination: WorkspaceDestination, key: KeyEquivalent
  ) -> some View {
    Button(title) {
      NotificationCenter.default.post(name: .selectSoyehtWorkspaceView, object: destination)
    }
    .keyboardShortcut(key, modifiers: .command)
  }
}

extension Notification.Name {
  static let showSoyehtPreferences = Notification.Name("showSoyehtPreferences")
  static let toggleSoyehtAppBar = Notification.Name("toggleSoyehtAppBar")
  static let selectSoyehtWorkspaceView = Notification.Name("selectSoyehtWorkspaceView")
  static let showSoyehtEditorPalette = Notification.Name("showSoyehtEditorPalette")
}
