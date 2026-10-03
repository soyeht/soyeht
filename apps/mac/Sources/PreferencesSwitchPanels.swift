import SoyehtClient
import SwiftUI

/// Preferences sections that hold one on/off choice.
struct WebLinkPreferencesPanel: View {
  @Environment(WebLinkPreferences.self) private var preferences

  var body: some View {
    PreferenceSwitchPanel(
      title: "Web links",
      summary: "Local links open in a web pane. Choose whether other web links do too.",
      label: "Open external links in Soyeht",
      detail: "When off, external links use the system browser",
      footnote: "localhost and loopback links always open in Soyeht.",
      isOn: preferences.openExternalLinksInSoyeht,
      toggle: { preferences.setOpenExternalLinksInSoyeht(!preferences.openExternalLinksInSoyeht) })
  }
}

struct EditorInputPreferencesPanel: View {
  @Environment(EditorInputPreferences.self) private var preferences

  var body: some View {
    PreferenceSwitchPanel(
      title: "Editor shortcuts",
      summary:
        "Choose how the editor interprets unmodified keys. This setting applies only to you.",
      label: "Vim mode",
      detail: "Normal, insert and visual modes with bounded Ex commands",
      footnote: "Off restores the standard editor bindings immediately.",
      isOn: preferences.vimEnabled,
      toggle: { preferences.setVimEnabled(!preferences.vimEnabled) })
  }
}

/// The shared layout of a Preferences section: heading, one switch row, footnote.
struct PreferenceSwitchPanel: View {
  let title: String
  let summary: String
  let label: String
  let detail: String
  let footnote: String
  let isOn: Bool
  let toggle: () -> Void

  var body: some View {
    PreferencePage(title: title, summary: summary) {
      Button(action: toggle) {
        HStack(spacing: MacTheme.Space.x14) {
          VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
            Text(label).font(MacTheme.chrome(PreferenceMetrics.rowTitleSize, weight: .bold))
            Text(detail)
              .font(MacTheme.chrome(MacTheme.TypeScale.control))
              .foregroundStyle(MacTheme.secondary)
          }
          Spacer()
          PreferenceToggle(isOn: isOn)
        }
        .padding(.horizontal, MacTheme.Space.x16).frame(height: PreferenceMetrics.rowHeight)
        .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
      }
      .buttonStyle(.plain)
      Text(footnote)
        .font(MacTheme.chrome(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
    }
  }
}

/// Sizes shared by Preferences detail pages.
enum PreferenceMetrics {
  static let rowHeight: CGFloat = 64
  static let rowTitleSize: CGFloat = 14
  static let sideInset: CGFloat = 22
}

/// A Preferences detail page: heading and summary above its content.
struct PreferencePage<Content: View>: View {
  let title: String
  let summary: String
  @ViewBuilder let content: Content

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      Text(summary)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      content
      Spacer()
    }
    .padding(.vertical, MacTheme.Space.x16).padding(.horizontal, PreferenceMetrics.sideInset)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}
