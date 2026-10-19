import SoyehtClient
import SwiftUI

/// The task toolbar's controls. `TaskToolbarDensity` decides which of them show and which carry
/// a label; neither ever wraps its text.
struct TaskViewToggle: View {
  @Binding var mode: TaskViewMode
  let showsLabels: Bool

  var body: some View {
    HStack(spacing: MacTheme.Space.x2) {
      choice(.board, icon: "square.grid.2x2")
      choice(.list, icon: "list.bullet")
    }
    .padding(MacTheme.Space.x3).background(MacTheme.well, in: Capsule())
  }

  private func choice(_ value: TaskViewMode, icon: String) -> some View {
    Button {
      mode = value
    } label: {
      Label {
        if showsLabels { Text(value.rawValue).lineLimit(1).fixedSize() }
      } icon: {
        Image(systemName: icon)
      }
      .help(value.rawValue)
      .font(
        MacTheme.chrome(MacTheme.TypeScale.control, weight: mode == value ? .bold : .semibold)
      )
      .foregroundStyle(mode == value ? MacTheme.ink : MacTheme.muted)
      .padding(.horizontal, MacTheme.Space.x12).padding(.vertical, MacTheme.Space.x6)
      .background {
        if mode == value {
          RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.face)
            .shadow(color: MacTheme.shadowDark, radius: 2, x: 2, y: 2)
            .shadow(color: MacTheme.shadowLight, radius: 2, x: -2, y: -2)
        }
      }
    }.buttonStyle(.plain)
  }
}

struct TaskToolbarLabel: View {
  let title: String
  let icon: String
  let trailingIcon: String?
  let accent: Bool
  let showsTitle: Bool

  init(
    _ title: String, icon: String, trailingIcon: String?, accent: Bool = false,
    showsTitle: Bool = true
  ) {
    self.title = title
    self.icon = icon
    self.trailingIcon = trailingIcon
    self.accent = accent
    self.showsTitle = showsTitle
  }

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(
        .system(size: DesignTokens.Glyph.taskToolbarIcon, weight: .semibold))
      if showsTitle {
        Text(title).font(
          MacTheme.chrome(MacTheme.TypeScale.control, weight: accent ? .bold : .semibold)
        ).lineLimit(1).fixedSize()
      }
      if let trailingIcon {
        Image(systemName: trailingIcon).font(
          .system(size: DesignTokens.Glyph.taskToolbarChevron, weight: .semibold)
        )
        .foregroundStyle(accent ? MacTheme.onAccent : MacTheme.muted)
      }
    }
    .foregroundStyle(accent ? MacTheme.onAccent : MacTheme.secondary)
    .padding(.leading, MacTheme.Space.x10).padding(.trailing, MacTheme.Space.x10)
    .padding(.vertical, MacTheme.Space.x6)
    .background {
      if accent {
        RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.accent)
          .shadow(color: MacTheme.accent.opacity(0.4), radius: 5, x: 4, y: 4)
          .shadow(color: MacTheme.shadowLight, radius: 3, x: -3, y: -3)
      } else {
        RoundedRectangle(cornerRadius: MacTheme.Radius.pill).fill(MacTheme.face)
          .shadow(color: MacTheme.shadowDark, radius: 2.5, x: 3, y: 3)
          .shadow(color: MacTheme.shadowLight, radius: 2.5, x: -3, y: -3)
      }
    }
  }
}
