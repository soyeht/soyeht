import SoyehtClient
import SwiftUI

@main
struct SoyehtMacApp: App {
  @State private var client = EngineClient()
  var body: some Scene {
    Window("Soyeht · Dev", id: "main") {
      WorkspaceWindow(client: client)
        .frame(minWidth: 850, minHeight: 550)
        .task { client.start() }
    }
    .defaultSize(width: 1180, height: 760)
    .windowStyle(.hiddenTitleBar)
    .commands { CommandGroup(replacing: .newItem) {} }
  }
}
