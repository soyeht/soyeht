import SoyehtClient
import SwiftUI

extension Notification.Name {
  static let splitSoyehtPane = Notification.Name("splitSoyehtPane")
  static let moveSoyehtPaneFocus = Notification.Name("moveSoyehtPaneFocus")
}

/// The Pane menu: split the focused pane and move the focus between panes, as tmux and iTerm do.
/// The window carries them out (`WorkspaceWindow`), which knows the focused pane and the room.
struct PaneCommands: Commands {
  var body: some Commands {
    CommandMenu("Pane") {
      split("Split Right", axis: .columns, key: "d", modifiers: .command)
      split("Split Down", axis: .rows, key: "d", modifiers: [.command, .shift])
      Divider()
      move("Focus Pane to the Left", .toward(.left), key: .leftArrow)
      move("Focus Pane to the Right", .toward(.right), key: .rightArrow)
      move("Focus Pane Above", .toward(.up), key: .upArrow)
      move("Focus Pane Below", .toward(.down), key: .downArrow)
      move("Focus Next Pane", .next, key: "]", modifiers: .command)
      move("Focus Previous Pane", .previous, key: "[", modifiers: .command)
    }
  }

  private func split(
    _ title: String, axis: PaneSplitAxis, key: KeyEquivalent, modifiers: EventModifiers
  ) -> some View {
    Button(title) { NotificationCenter.default.post(name: .splitSoyehtPane, object: axis) }
      .keyboardShortcut(key, modifiers: modifiers)
  }

  private func move(
    _ title: String, _ move: PaneFocusMove, key: KeyEquivalent,
    modifiers: EventModifiers = [.command, .option]
  ) -> some View {
    Button(title) { NotificationCenter.default.post(name: .moveSoyehtPaneFocus, object: move) }
      .keyboardShortcut(key, modifiers: modifiers)
  }
}
