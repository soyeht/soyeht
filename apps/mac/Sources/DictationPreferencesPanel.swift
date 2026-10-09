import SoyehtClient
import SwiftUI

/// Preferences > Dictation: the engine behind the composer's mic, its language and vocabulary
/// (ADR 0083). The page is generated from the engine catalogue.
struct DictationPreferencesPanel: View {
  var body: some View {
    PreferencePage(title: "Dictation", summary: DictationPreferencesView.summary) {
      ScrollView {
        DictationPreferencesView(store: .shared, style: MacTheme.agentConversation)
      }
    }
  }
}

extension View {
  /// Runs `open` when the dictation shortcut found only one engine installed and points at an
  /// install step: now, if that happened before this view appeared, and whenever it happens.
  func dictationInstallPrompt(_ open: @escaping () -> Void) -> some View {
    onChange(of: DictationEngineStore.shared.installPrompt, initial: true) { _, prompt in
      if prompt != nil { open() }
    }
  }
}
