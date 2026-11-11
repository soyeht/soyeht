import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct ThemePreferencesPanel: View {
  @Environment(SoyehtThemeStore.self) private var themeStore

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        Text("Appearance")
          .font(MacTheme.chrome(18, weight: .bold))
        Text("Choose one neomorphic theme for every Soyeht window and pane.")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      }
      ScrollView {
        LazyVGrid(
          columns: [GridItem(.flexible()), GridItem(.flexible())],
          spacing: MacTheme.Space.x12
        ) {
          ForEach(TerminalColorTheme.allCases, id: \.rawValue) { theme in
            ThemePreferenceCard(
              theme: theme,
              selected: themeStore.activeTheme == theme,
              select: { themeStore.select(theme) })
          }
        }
        .padding(MacTheme.Space.x4)
      }
      Text("Your choice applies immediately and is remembered on this device.")
        .font(MacTheme.chrome(MacTheme.TypeScale.small))
        .foregroundStyle(MacTheme.muted)
    }
    .padding(.vertical, MacTheme.Space.x16)
    .padding(.horizontal, MacTheme.Space.x22)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}

private struct ThemePreferenceCard: View {
  let theme: TerminalColorTheme
  let selected: Bool
  let select: () -> Void

  var body: some View {
    Button(action: select) {
      HStack(spacing: MacTheme.Space.x12) {
        ThemePreviewSwatch(theme: theme)
        Text(theme.displayName)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: selected ? .bold : .semibold))
          .foregroundStyle(MacTheme.ink)
          .lineLimit(1)
        Spacer(minLength: 0)
        Image(systemName: selected ? "checkmark.circle.fill" : "circle")
          .font(.system(size: MacTheme.TypeScale.body, weight: .semibold))
          .foregroundStyle(selected ? MacTheme.accent : MacTheme.muted)
      }
      .padding(MacTheme.Space.x12)
      .frame(maxWidth: .infinity, minHeight: 76, alignment: .leading)
      .background(selected ? MacTheme.well : MacTheme.face)
      .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous))
      .overlay {
        RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous)
          .stroke(selected ? MacTheme.accent : MacTheme.well, lineWidth: selected ? 2 : 1)
      }
    }
    .buttonStyle(.plain)
    .accessibilityLabel(theme.displayName)
    .accessibilityValue(selected ? "Selected" : "Not selected")
    .accessibilityAddTraits(selected ? .isSelected : [])
  }
}

private struct ThemePreviewSwatch: View {
  let theme: TerminalColorTheme

  private var colors: SoyehtThemeColorSet { SoyehtThemeColorSet(palette: theme.palette) }

  var body: some View {
    ZStack {
      RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous)
        .fill(colors.canvas)
      raisedPlate
      wellSample
    }
    .frame(width: 88, height: 50)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control, style: .continuous))
    .accessibilityHidden(true)
  }

  private var raisedPlate: some View {
    RoundedRectangle(cornerRadius: MacTheme.Radius.compact, style: .continuous)
      .fill(colors.face)
      .frame(width: 48, height: 28)
      .overlay(alignment: .leading) {
        Capsule().fill(colors.accent).frame(width: 6, height: 18).padding(
          .leading, MacTheme.Space.x8)
      }
      .shadow(color: colors.shadowLight, radius: 3.5, x: -2, y: -2)
      .shadow(color: colors.shadowDark, radius: 4, x: 2, y: 2)
      .offset(x: -12)
  }

  private var wellSample: some View {
    Circle()
      .fill(
        colors.well
          .shadow(.inner(color: colors.wellShadow, radius: 2.5, x: 2, y: 2))
          .shadow(.inner(color: colors.wellRim, radius: 2.5, x: -2, y: -2))
      )
      .overlay {
        Circle().fill(colors.ink).frame(width: 5, height: 5)
      }
      .frame(width: 22, height: 22)
      .offset(x: 26)
  }
}
