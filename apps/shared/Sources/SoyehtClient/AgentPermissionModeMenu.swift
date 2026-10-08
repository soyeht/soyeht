import SwiftUI

/// The composer's permission ("rédeas") menu: a shield, the harness's name for the confirmed
/// mode (or a spinner and the requested one while the harness has not confirmed it) and a
/// chevron. A mode that approves everything is drawn in `warning`.
struct AgentPermissionModeMenu: View {
  let modes: AgentPermissionModes
  let enabled: Bool
  let style: AgentConversationStyle
  let select: (String) -> Void

  var body: some View {
    Menu {
      AgentPermissionModeItems(modes: modes, select: select)
    } label: {
      AgentComposerChip(title: title, emphasized: false, style: style) {
        if modes.pending != nil {
          ProgressView().controlSize(.mini)
        } else {
          Image(systemName: fullAccess ? "exclamationmark.shield" : "shield")
            .font(.system(size: AgentComposerMetrics.shieldSize - 2, weight: .medium))
            .foregroundStyle(fullAccess ? style.warning : style.secondary)
            .frame(
              width: AgentComposerMetrics.shieldSize, height: AgentComposerMetrics.shieldSize)
        }
      }
    }
    .composerMenuStyle()
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
      .help(mode.detail ?? "")
    }
  }

  /// The chosen mode's name, else the preferred mode's, else "Harness default".
  public static func title(
    modes: AgentPermissionModes?, selection: String?, preferred: String?
  ) -> String {
    modes?.mode(selection ?? preferred)?.name ?? "Harness default"
  }
}

/// The items of the composer's permission menu.
struct AgentPermissionModeItems: View {
  let modes: AgentPermissionModes
  let select: (String) -> Void

  var body: some View {
    ForEach(modes.modes) { mode in
      Button {
        select(mode.id)
      } label: {
        PermissionModeItem(mode: mode, current: mode.id == modes.current)
      }
      .help(mode.detail ?? "")
      .disabled(mode.id == modes.current)
    }
  }
}

/// One menu item: the harness's name for the mode and a warning mark on modes that approve
/// everything. The description is the item's tooltip, never a subtitle: AppKit raised an
/// exception building a title-and-subtitle item while SwiftUI refreshed the open menu.
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
  }
}
