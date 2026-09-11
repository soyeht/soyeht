import SwiftUI

@main
struct SoyehtPhoneApp: App {
  @State private var model = PhoneModel()
  @Environment(\.scenePhase) private var phase
  var body: some Scene {
    WindowGroup {
      PhoneHome(model: model)
        .task { model.resume() }
        .onChange(of: phase) { _, phase in
          if phase == .active { model.resume() }
          if phase == .background { model.client?.stop() }
        }
        .onOpenURL { model.pair($0.absoluteString) }
    }
  }
}
