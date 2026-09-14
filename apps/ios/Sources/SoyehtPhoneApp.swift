import SoyehtClient
import SwiftUI

@main @MainActor
struct SoyehtPhoneApp: App {
  @State private var model = PhoneModel()
  @State private var themeStore = SoyehtThemeStore.shared
  @Environment(\.scenePhase) private var phase
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
        .onOpenURL { model.pair($0.absoluteString) }
    }
  }
}
