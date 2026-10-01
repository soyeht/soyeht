import SwiftUI

/// What the pane keyboard shortcuts can do in the active scene. The workspace split
/// publishes it while it is on screen; the menu stays disabled without it.
@MainActor struct PhonePaneNavigation {
  let workspaceNames: [String]
  let step: @MainActor (Int) -> Void
  let showWorkspace: @MainActor (Int) -> Void
  let focusSidebar: @MainActor () -> Void
}

private struct PhonePaneNavigationKey: FocusedValueKey {
  typealias Value = PhonePaneNavigation
}

extension FocusedValues {
  var phonePaneNavigation: PhonePaneNavigation? {
    get { self[PhonePaneNavigationKey.self] }
    set { self[PhonePaneNavigationKey.self] = newValue }
  }
}

/// Hardware keyboard shortcuts, also listed in the iPad menu bar and the Command-key overlay.
struct PhonePaneCommands: Commands {
  @FocusedValue(\.phonePaneNavigation) private var navigation

  var body: some Commands {
    CommandMenu("Panes") {
      Button("Next Pane") { navigation?.step(1) }
        .keyboardShortcut("]", modifiers: .command)
        .disabled(navigation == nil)
      Button("Previous Pane") { navigation?.step(-1) }
        .keyboardShortcut("[", modifiers: .command)
        .disabled(navigation == nil)
      Button("Focus Sidebar") { navigation?.focusSidebar() }
        .keyboardShortcut("s", modifiers: [.command, .control])
        .disabled(navigation == nil)
      Divider()
      ForEach(Array((navigation?.workspaceNames ?? []).prefix(9).enumerated()), id: \.offset) {
        index, name in
        Button(name) { navigation?.showWorkspace(index) }
          .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: .command)
      }
    }
  }
}
