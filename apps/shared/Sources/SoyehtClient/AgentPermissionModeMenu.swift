import SwiftUI

/// Compact menu of the harness's own permission modes for an agent pane
/// header. The label shows the confirmed mode, or a spinner and the requested
/// mode while the harness has not confirmed it yet.
struct AgentPermissionModeMenu: View {
  let modes: AgentPermissionModes
  let enabled: Bool
  let style: AgentConversationStyle
  let select: (String) -> Void

  var body: some View {
    Menu {
      ForEach(modes.modes) { mode in
        Button {
          select(mode.id)
        } label: {
          PermissionModeItem(mode: mode, current: mode.id == modes.current)
        }
        .disabled(mode.id == modes.current)
      }
    } label: {
      HStack(spacing: 5) {
        if modes.pending != nil {
          ProgressView().controlSize(.mini)
        } else {
          Image(systemName: fullAccess ? "exclamationmark.shield" : "checkmark.shield")
        }
        Text(title).lineLimit(1).truncationMode(.tail)
      }
    }
    .menuStyle(.button)
    .buttonStyle(.bordered)
    .tint(fullAccess ? style.warning : style.secondary)
    .disabled(!enabled || modes.pending != nil)
    .accessibilityLabel("Permission mode")
    .accessibilityValue(accessibilityValue)
    .accessibilityHint("Shift-Tab in the message field moves to the next mode")
    .accessibilityIdentifier("agent-permission-mode")
  }

  private var fullAccess: Bool { modes.currentMode?.fullAccess == true }

  private var title: String {
    (modes.pendingMode ?? modes.currentMode)?.name ?? modes.pending ?? modes.current ?? "Mode"
  }

  private var accessibilityValue: String {
    modes.pending == nil ? title : "Applying \(title)"
  }
}

/// Menu items that choose a permission mode before a pane exists: the
/// harness's preferred mode (`nil`) or one of the modes it announced.
public struct PermissionModeChoices: View {
  let modes: AgentPermissionModes?
  @Binding var selection: String?

  public init(modes: AgentPermissionModes?, selection: Binding<String?>) {
    self.modes = modes
    _selection = selection
  }

  public var body: some View {
    Button("Harness default") { selection = nil }
    ForEach(modes?.modes ?? []) { mode in
      Button {
        selection = mode.id
      } label: {
        PermissionModeItem(mode: mode, current: mode.id == selection)
      }
    }
  }

  /// The chosen mode's name, else the preferred mode's, else "Harness default".
  public static func title(
    modes: AgentPermissionModes?, selection: String?, preferred: String?
  ) -> String {
    modes?.mode(selection ?? preferred)?.name ?? "Harness default"
  }
}

/// One menu item: the harness's name for the mode, its description as a
/// subtitle, and a warning mark on modes that approve everything.
private struct PermissionModeItem: View {
  let mode: AgentPermissionMode
  let current: Bool

  var body: some View {
    if current {
      Label(mode.name, systemImage: "checkmark")
    } else if mode.fullAccess {
      Label(mode.name, systemImage: "exclamationmark.shield")
    } else {
      Text(mode.name)
    }
    if let detail = mode.detail { Text(detail) }
  }
}
