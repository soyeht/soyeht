import SoyehtClient
import SwiftUI

@main
struct SoyehtMacApp: App {
  @State private var client = EngineClient()

  init() {
    MacTheme.registerFonts()
  }

  var body: some Scene {
    Window("Soyeht · Dev", id: "main") {
      WorkspaceWindow(client: client)
        .frame(minWidth: 1024, minHeight: 640)
        .task { client.start() }
    }
    .defaultSize(width: 1440, height: 900)
    .windowStyle(.hiddenTitleBar)
    .commands { CommandGroup(replacing: .newItem) {} }
  }
}
