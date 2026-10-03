import SwiftUI

/// Compact menu of the harness's own permission modes in the agent composer. The flat
/// label reads "High · Accept Edits ▾": the reasoning effort, when the harness reports one,
/// then the confirmed mode, or a spinner and the requested mode while the harness has not
/// confirmed it yet. A mode that approves everything is drawn in `warning`.
struct AgentPermissionModeMenu: View {
  let modes: AgentPermissionModes
  var effort: String? = nil
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
      HStack(spacing: PaneInteriorMetrics.tightSpacing) {
        if modes.pending != nil {
          ProgressView().controlSize(.mini)
        } else if fullAccess {
          Image(systemName: "exclamationmark.shield")
        }
        Text(label).lineLimit(1).truncationMode(.tail)
      }
      .font(style.monoSmallFont.weight(.semibold))
      .foregroundStyle(fullAccess ? style.warning : style.ink)
      .contentShape(Rectangle())
    }
    .menuStyle(.button)
    .buttonStyle(.plain)
    .menuIndicator(.hidden)
    .fixedSize()
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

  private var label: String {
    ([effort, title].compactMap { $0 }.joined(separator: " · ")) + " ▾"
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
