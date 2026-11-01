import SoyehtClient
import SoyehtDictation
import SwiftUI

@main @MainActor
struct SoyehtPhoneApp: App {
  @UIApplicationDelegateAdaptor(PhoneAppDelegate.self) private var delegate
  @State private var themeStore = SoyehtThemeStore.shared
  @Environment(\.scenePhase) private var phase
  private var model: PhoneModel { delegate.model }

  init() {
    DictationEngineStore.shared.install(DictationCatalogue.registry)
  }

  var body: some Scene {
    WindowGroup {
      PhoneHome(model: model)
        .environment(themeStore)
        .preferredColorScheme(themeStore.activeTheme.palette.isDark ? .dark : .light)
        .task { model.resume() }
        .onChange(of: phase) { _, phase in
          if phase == .active { model.resume() }
          if phase == .background { model.client?.stop() }
        }
        .onChange(of: model.client?.connectionRevision) {
          // Asked once the paired Mac is reachable, never at launch.
          guard let client = model.client, client.connected else { return }
          Task { await model.push.connected(client) }
        }
        .onChange(of: model.client?.openRequestIds) { _, open in
          guard let open, model.client?.connected == true else { return }
          Task { await PhonePush.removeSettled(open: open) }
        }
        .onOpenURL { model.receive(link: $0.absoluteString) }
    }
    .commands { PhonePaneCommands() }
  }
}
