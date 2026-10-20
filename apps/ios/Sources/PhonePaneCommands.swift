import SoyehtClient
import SwiftUI

/// What the pane keyboard shortcuts can do in the active scene. The workspace split
/// publishes it while it is on screen; the menu stays disabled without it.
@MainActor struct PhonePaneNavigation {
  let workspaceNames: [String]
  let step: @MainActor (Int) -> Void
  let showWorkspace: @MainActor (Int) -> Void
  let focusSidebar: @MainActor () -> Void
}

/// What the keys can do while a pane board is on screen: step through its panes in visual
/// order, or move to the pane beside the focused one.
@MainActor struct PhonePaneBoardNavigation {
  let step: @MainActor (Int) -> Void
  let move: @MainActor (PaneBoardDirection) -> Void
}

private struct PhonePaneNavigationKey: FocusedValueKey {
  typealias Value = PhonePaneNavigation
}

private struct PhonePaneBoardNavigationKey: FocusedValueKey {
  typealias Value = PhonePaneBoardNavigation
}

extension FocusedValues {
  var phonePaneNavigation: PhonePaneNavigation? {
    get { self[PhonePaneNavigationKey.self] }
    set { self[PhonePaneNavigationKey.self] = newValue }
  }

  var phonePaneBoardNavigation: PhonePaneBoardNavigation? {
    get { self[PhonePaneBoardNavigationKey.self] }
    set { self[PhonePaneBoardNavigationKey.self] = newValue }
  }
}

/// Hardware keyboard shortcuts, also listed in the iPad menu bar and the Command-key overlay.
struct PhonePaneCommands: Commands {
  @FocusedValue(\.phonePaneNavigation) private var navigation
  @FocusedValue(\.phonePaneBoardNavigation) private var board

  var body: some Commands {
    CommandMenu("Panes") {
      Button("Next Pane") { step(1) }
        .keyboardShortcut("]", modifiers: .command)
        .disabled(navigation == nil)
      Button("Previous Pane") { step(-1) }
        .keyboardShortcut("[", modifiers: .command)
        .disabled(navigation == nil)
      Button("Focus Sidebar") { navigation?.focusSidebar() }
        .keyboardShortcut("s", modifiers: [.command, .control])
        .disabled(navigation == nil)
      Divider()
      Button("Pane to the Left") { board?.move(.left) }
        .keyboardShortcut(.leftArrow, modifiers: [.command, .option])
        .disabled(board == nil)
      Button("Pane to the Right") { board?.move(.right) }
        .keyboardShortcut(.rightArrow, modifiers: [.command, .option])
        .disabled(board == nil)
      Button("Pane Above") { board?.move(.up) }
        .keyboardShortcut(.upArrow, modifiers: [.command, .option])
        .disabled(board == nil)
      Button("Pane Below") { board?.move(.down) }
        .keyboardShortcut(.downArrow, modifiers: [.command, .option])
        .disabled(board == nil)
      Divider()
      ForEach(Array((navigation?.workspaceNames ?? []).prefix(9).enumerated()), id: \.offset) {
        index, name in
        Button(name) { navigation?.showWorkspace(index) }
          .keyboardShortcut(KeyEquivalent(Character("\(index + 1)")), modifiers: .command)
      }
    }
  }

  /// On a board the keys walk the board's panes; otherwise the sidebar's list.
  private func step(_ offset: Int) {
    if let board { board.step(offset) } else { navigation?.step(offset) }
  }
}
