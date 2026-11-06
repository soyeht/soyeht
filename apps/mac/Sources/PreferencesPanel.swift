import SoyehtClient
import SoyehtProtocol
import SwiftUI

private enum PreferencesNavigationItem: CaseIterable, Identifiable {
  case general
  case appearance
  case harnesses
  case usage
  case links
  case editor
  case skills
  case dictation

  var id: Self { self }

  var title: String {
    switch self {
    case .general: "General"
    case .appearance: "Appearance"
    case .harnesses: "Harnesses"
    case .usage: "Usage"
    case .editor: "Shortcuts"
    case .links: "Links"
    case .skills: "Skills"
    case .dictation: "Dictation"
    }
  }

  var icon: String {
    switch self {
    case .general: "gearshape"
    case .appearance: "paintpalette"
    case .harnesses: "cpu"
    case .usage: "gauge.with.dots.needle.50percent"
    case .editor: "keyboard"
    case .links: "link"
    case .skills: "wand.and.stars"
    case .dictation: "mic"
    }
  }
}

struct PreferencesPanel: View {
  @Bindable var client: EngineClient
  @State private var section: PreferencesNavigationItem
  @State private var compactSection: PreferencesNavigationItem?
  @State private var selectedUsageProvider: UsageProvider?

  init(client: EngineClient, initialUsageProvider: UsageProvider? = nil) {
    self.client = client
    _section = State(initialValue: initialUsageProvider == nil ? .harnesses : .usage)
    _compactSection = State(initialValue: initialUsageProvider == nil ? nil : .usage)
    _selectedUsageProvider = State(initialValue: initialUsageProvider)
  }

  var body: some View {
    MacSheetSurface(kind: .preferences) { layoutPlan in
      Group {
        if layoutPlan.navigation == .stack {
          compactPreferences
        } else {
          HStack(spacing: MacTheme.Space.x10) {
            PreferencesSidebar(client: client, selection: $section)
            preferenceDetail(section)
          }
        }
      }
      .padding(MacTheme.Space.x10)
      .foregroundStyle(MacTheme.ink)
      .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
      .onAppear { refreshCatalogues() }
      .dictationInstallPrompt {
        section = .dictation
        compactSection = .dictation
      }
      .onChange(of: client.harnesses.map(\.id)) { _, _ in refreshCatalogues() }
    }
  }

  @ViewBuilder private var compactPreferences: some View {
    if let compactSection {
      VStack(spacing: MacTheme.Space.x10) {
        HStack(spacing: MacTheme.Space.x10) {
          Button {
            self.compactSection = nil
          } label: {
            HStack(spacing: MacTheme.Space.x10) {
              Image(systemName: "chevron.left")
                .frame(width: CGFloat(MacSheetLayoutMetrics.standard.compactNavigationIconWidth))
              Text("Preferences")
            }
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
            .frame(minHeight: CGFloat(MacSheetLayoutMetrics.standard.compactNavigationRowHeight))
            .contentShape(Rectangle())
          }
          .buttonStyle(.plain)
          .accessibilityIdentifier("preferences-back")
          Spacer()
          Text(compactSection.title)
            .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        }
        .padding(.horizontal, MacTheme.Space.x8)
        preferenceDetail(compactSection)
      }
    } else {
      ScrollView(.vertical) {
        VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
          Text("Preferences")
            .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
            .padding(.horizontal, MacTheme.Space.x8)
          ForEach(PreferencesNavigationItem.allCases) { item in
            compactButton(item)
          }
        }
        .padding(MacTheme.Space.x8)
      }
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
  }

  private func compactButton(_ candidate: PreferencesNavigationItem) -> some View {
    Button {
      section = candidate
      compactSection = candidate
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: candidate.icon)
          .frame(width: CGFloat(MacSheetLayoutMetrics.standard.compactNavigationIconWidth))
        Text(candidate.title)
        Spacer()
        Image(systemName: "chevron.right")
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
      .foregroundStyle(MacTheme.ink)
      .padding(.horizontal, MacTheme.Space.x12)
      .frame(
        maxWidth: .infinity,
        minHeight: CGFloat(MacSheetLayoutMetrics.standard.compactNavigationRowHeight)
      )
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
  }

  @ViewBuilder private func preferenceDetail(_ selected: PreferencesNavigationItem) -> some View {
    switch selected {
    case .general: GeneralPreferencesPanel(client: client)
    case .appearance: ThemePreferencesPanel()
    case .harnesses: PreferencesHarnesses(client: client)
    case .usage:
      UsageSettingsDetail(client: client, selectedProvider: $selectedUsageProvider)
    case .editor: EditorInputPreferencesPanel()
    case .links: WebLinkPreferencesPanel()
    case .skills: SkillsPreferencesPanel(client: client)
    case .dictation: DictationPreferencesPanel()
    }
  }

  private func refreshCatalogues() {
    client.refreshHarnesses()
    client.refreshDefaultPermissionModes()
    for harness in client.harnesses where harness.available {
      client.refreshCatalogue(harness: harness.id)
    }
  }
}

private struct PreferencesSidebar: View {
  @Bindable var client: EngineClient
  @Binding var selection: PreferencesNavigationItem

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      Text("Preferences")
        .font(MacTheme.chrome(16, weight: .bold))
        .padding(.horizontal, MacTheme.Space.x4)
      Color.clear.frame(height: MacTheme.Space.x8)
      ForEach(PreferencesNavigationItem.allCases) { item in
        sidebarButton(item)
      }
      Spacer()
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text(ActiveProfile.values.versionLine(version: appVersion))
        Text(client.connected ? "Engine connected · ACP v1" : "Engine reconnecting · ACP v1")
      }
      .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
      .foregroundStyle(MacTheme.muted)
      .padding(.horizontal, MacTheme.Space.x12)
      .padding(.vertical, MacTheme.Space.x8)
    }
    .padding(.vertical, MacTheme.Space.x12)
    .padding(.horizontal, MacTheme.Space.x8)
    .frame(width: 220)
    .frame(maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }

  private func sidebarRow(_ item: PreferencesNavigationItem, selected: Bool) -> some View {
    HStack(spacing: MacTheme.Space.x10) {
      Image(systemName: item.icon)
        .font(.system(size: MacTheme.TypeScale.caption, weight: .semibold))
        .frame(width: 15, height: 15)
        .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
      Text(item.title)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: selected ? .bold : .semibold))
        .foregroundStyle(selected ? MacTheme.ink : MacTheme.secondary)
      Spacer()
    }
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: 34)
    .background(
      selected ? MacTheme.well : Color.clear,
      in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
    )
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(selected ? .isSelected : [])
  }

  private func sidebarButton(_ section: PreferencesNavigationItem) -> some View {
    Button {
      selection = section
    } label: {
      sidebarRow(section, selected: selection == section)
    }
    .buttonStyle(.plain)
  }

  private var appVersion: String {
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Dev"
  }
}

struct PreferenceChip: View {
  let icon: String
  let text: String

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: MacTheme.TypeScale.micro, weight: .semibold))
      Text(text)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .lineLimit(1)
      Image(systemName: "chevron.down")
        .font(.system(size: DesignTokens.Glyph.chipChevron, weight: .bold))
        .foregroundStyle(MacTheme.muted)
    }
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x8)
    .frame(height: 26)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
    .accessibilityElement(children: .combine)
  }
}

struct PreferenceToggle: View {
  let isOn: Bool

  var body: some View {
    HStack {
      if isOn { Spacer(minLength: 0) }
      Circle().fill(MacTheme.onAccent).frame(width: 18, height: 18)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .toggle)
      if !isOn { Spacer(minLength: 0) }
    }
    .padding(MacTheme.Space.x2)
    .frame(width: 40, height: 22)
    .background(isOn ? MacTheme.accent : MacTheme.well, in: Capsule())
    .accessibilityHidden(true)
  }
}
