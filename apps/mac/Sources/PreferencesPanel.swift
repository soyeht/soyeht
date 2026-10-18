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
    case .general: GeneralPreferencesPanel()
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
        Text("Soyeht \(appVersion) · Dev profile")
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
        .font(.system(size: 13, weight: .semibold))
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

private struct ThemePreferencesPanel: View {
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
    .padding(.horizontal, 22)
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
          .font(.system(size: 15, weight: .semibold))
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
        Capsule().fill(colors.accent).frame(width: 6, height: 18).padding(.leading, 8)
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

private struct PreferencesHarnesses: View {
  @Bindable var client: EngineClient
  @State private var showingEditor = false
  @State private var editingHarness: HarnessLaunch?
  @State private var pendingRemoval: HarnessLaunch?

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      header
      Text(
        "Each harness runs as an engine subprocess and communicates over ACP. A raw terminal remains available for every command-line agent."
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.caption))
      .foregroundStyle(MacTheme.secondary)
      .lineSpacing(4)
      if let issue = client.harnessEnvironmentIssue {
        Label(environmentIssueText(issue), systemImage: "exclamationmark.triangle")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
          .foregroundStyle(MacTheme.warning)
          .accessibilityLabel("Harness setup: \(environmentIssueText(issue))")
      }
      ScrollView {
        VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
          ForEach(client.harnesses, id: \.id) { harness in
            HarnessPreferenceRow(
              harness: harness, catalogue: client.agentCatalogues[harness.id],
              isDefault: harness.id
                == HarnessPickerModel.defaultHarness(
                  in: client.harnesses, preferredId: client.preferredHarnessId)?.id,
              customLaunch: client.customHarnesses.first { $0.id == harness.id },
              defaultMode: Binding(
                get: { client.defaultPermissionMode(harness: harness.id) },
                set: { setDefaultMode($0, harness: harness.id) }),
              makeDefault: { makeDefault(harness.id) },
              edit: edit, remove: { pendingRemoval = $0 })
          }
          HarnessListLoadRows(client: client)
          Text("RAW TERMINAL")
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
            .tracking(1)
            .foregroundStyle(MacTheme.muted)
            .padding(.top, MacTheme.Space.x2)
          RawTerminalPreferenceRow()
        }
        .padding(.horizontal, MacTheme.Space.x4)
        .padding(.vertical, MacTheme.Space.x4)
      }
    }
    .padding(.vertical, MacTheme.Space.x16)
    .padding(.horizontal, 22)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    .onAppear { client.refreshCustomHarnesses() }
    .sheet(isPresented: $showingEditor) {
      CustomHarnessEditor(
        client: client, original: editingHarness, isPresented: $showingEditor)
    }
    .confirmationDialog(
      "Remove \(pendingRemoval?.name ?? "custom harness")?",
      isPresented: Binding(
        get: { pendingRemoval != nil },
        set: { if !$0 { pendingRemoval = nil } })
    ) {
      Button("Remove", role: .destructive) { removePendingHarness() }
      Button("Cancel", role: .cancel) { pendingRemoval = nil }
    } message: {
      Text("Open panes using this harness must be closed first.")
    }
  }

  private func setDefaultMode(_ mode: String?, harness: String) {
    Task { @MainActor in
      do { try await client.setDefaultPermissionMode(mode, harness: harness) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private func makeDefault(_ harnessId: String) {
    Task { @MainActor in
      do { try await client.setPreferredHarness(harnessId) } catch {
        client.problem = error.localizedDescription
      }
    }
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Text("Harnesses").font(MacTheme.chrome(18, weight: .bold))
      Spacer()
      Button {
        editingHarness = nil
        showingEditor = true
      } label: {
        Label("Add custom", systemImage: "plus")
      }
      .buttonStyle(NeoPillButtonStyle())
      Button {
        client.refreshHarnesses(rescan: true)
      } label: {
        Label("Rescan", systemImage: "arrow.clockwise")
      }
      .buttonStyle(NeoPillButtonStyle())
      .help("Read the login-shell environment again and refresh installed harnesses")
    }
  }

  private func environmentIssueText(_ issue: HarnessEnvironmentIssue) -> String {
    switch issue {
    case .missing_shell: return "No login shell is configured; using the app environment."
    case .unsupported_shell: return "The configured login shell is not supported."
    case .unavailable_shell: return "The configured login shell could not be started."
    case .probe_failed: return "The login-shell environment could not be read."
    case .probe_timed_out: return "Reading the login-shell environment timed out."
    case .output_too_large: return "The login shell produced too much startup output."
    case .invalid_output: return "The login shell returned an invalid environment."
    }
  }

  private func edit(_ harness: HarnessLaunch) {
    editingHarness = harness
    showingEditor = true
  }

  private func removePendingHarness() {
    guard let harness = pendingRemoval else { return }
    pendingRemoval = nil
    Task {
      do { try await client.removeCustomHarness(harness.id) } catch {
        client.problem = error.localizedDescription
      }
    }
  }
}

private struct HarnessPreferenceRow: View {
  let harness: HarnessInfo
  let catalogue: AgentCatalogue?
  let isDefault: Bool
  let customLaunch: HarnessLaunch?
  /// The mode new panes of this harness start in; `nil` keeps the harness's own.
  @Binding var defaultMode: String?
  let makeDefault: () -> Void
  let edit: (HarnessLaunch) -> Void
  let remove: (HarnessLaunch) -> Void
  @State private var showingStatus = false

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      identity
      HStack(spacing: MacTheme.Space.x8) {
        PreferenceChip(icon: "cpu", text: modelLabel)
        PreferenceChip(icon: "gauge.with.dots.needle.50percent", text: effortLabel)
        Menu {
          PermissionModeChoices(modes: permissionModes, selection: $defaultMode)
        } label: {
          PreferenceChip(
            icon: "checkmark.shield",
            text: PermissionModeChoices.title(
              modes: permissionModes, selection: defaultMode, preferred: nil))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("Permission mode new \(harness.name) panes start in")
        .accessibilityLabel("Default permission mode for \(harness.name)")
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      status
      if let customLaunch {
        Menu {
          Button("Edit") { edit(customLaunch) }
          Button("Remove", role: .destructive) { remove(customLaunch) }
        } label: {
          Image(systemName: "ellipsis")
            .font(.system(size: 14, weight: .semibold))
            .foregroundStyle(MacTheme.muted)
        }
        .menuStyle(.borderlessButton)
        .accessibilityLabel("Manage \(harness.name)")
      }
    }
    .preferenceRowSurface()
  }

  private var identity: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
      HStack(spacing: MacTheme.Space.x8) {
        Text(harness.name)
          .font(MacTheme.chrome(14, weight: .bold))
          .lineLimit(1)
        if isDefault {
          Text("default")
            .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
            .foregroundStyle(MacTheme.secondary)
            .padding(.horizontal, MacTheme.Space.x7)
            .padding(.vertical, 1)
            .background(MacTheme.well, in: Capsule())
        }
      }
      Text(versionLabel)
        .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
        .foregroundStyle(MacTheme.muted)
        .lineLimit(1)
    }
    .frame(width: 160, alignment: .leading)
  }

  private var status: some View {
    Button {
      showingStatus = true
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Circle().fill(statusColor).frame(width: 7, height: 7)
        Text(HarnessPickerModel.statusLabel(for: harness))
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
          .foregroundStyle(MacTheme.secondary)
      }
      .frame(width: 118, alignment: .leading)
      .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel("\(harness.name): \(HarnessPickerModel.statusLabel(for: harness))")
    .popover(isPresented: $showingStatus) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
        Text(HarnessPickerModel.statusLabel(for: harness))
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        Text(HarnessPickerModel.statusReason(for: harness))
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .fixedSize(horizontal: false, vertical: true)
        if harness.available && !isDefault {
          Button("Make default") {
            makeDefault()
            showingStatus = false
          }
          .buttonStyle(NeoPillButtonStyle())
        }
      }
      .padding(MacTheme.Space.x16)
      .frame(width: 260, alignment: .leading)
    }
  }

  private var versionLabel: String {
    guard let version = harness.version, !version.isEmpty else {
      return "Version unavailable · ACP"
    }
    return "\(version) · ACP"
  }

  private var modelLabel: String {
    if let option = options.first(where: \.isModel),
      case .v0(let value) = option.currentValue
    {
      return option.options.first(where: { $0.value == value })?.name ?? value
    }
    return catalogue?.models.first?.name ?? "Agent default"
  }

  private var effortLabel: String {
    guard
      let option = options.first(where: \.isEffort), case .v0(let value) = option.currentValue
    else { return "Default effort" }
    return option.options.first(where: { $0.value == value })?.name ?? value.capitalized
  }

  private var options: [ConfigOption] { catalogue?.options ?? harness.options }

  private var permissionModes: AgentPermissionModes? { AgentPermissionModes(options: options) }

  private var statusColor: Color {
    switch HarnessPickerModel.setupState(for: harness) {
    case .ready: MacTheme.success
    case .sign_in, .check_setup: MacTheme.warning
    case .not_checked, .not_installed: MacTheme.muted
    }
  }
}

private struct CustomHarnessEditor: View {
  @Bindable var client: EngineClient
  let original: HarnessLaunch?
  @Binding var isPresented: Bool
  @State private var name: String
  @State private var command: String
  @State private var arguments: String
  @State private var version: String
  @State private var environment: String
  @State private var submitting = false
  @State private var failure: String?

  init(client: EngineClient, original: HarnessLaunch?, isPresented: Binding<Bool>) {
    self.client = client
    self.original = original
    _isPresented = isPresented
    _name = State(initialValue: original?.name ?? "")
    _command = State(initialValue: original?.command ?? "")
    _arguments = State(initialValue: original?.args.joined(separator: "\n") ?? "")
    _version = State(initialValue: original?.version ?? "custom")
    _environment = State(
      initialValue: original?.environment?.map { "\($0.key)=\($0.value)" }.joined(separator: "\n")
        ?? "")
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      HStack {
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(original == nil ? "Add custom harness" : "Edit custom harness")
            .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          Text("Use an installed ACP-compatible command. No install command is ever run.")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption))
            .foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Button("Cancel") { isPresented = false }.buttonStyle(NeoPillButtonStyle())
      }
      field("DISPLAY NAME · REQUIRED", text: $name, prompt: "My agent")
      field("COMMAND · REQUIRED", text: $command, prompt: "agent-command")
      field("VERSION · REQUIRED", text: $version, prompt: "custom")
      editor(
        "ARGUMENTS · ONE PER LINE", text: $arguments,
        prompt: "acp\n--optional-flag")
      editor(
        "ENVIRONMENT · KEY=VALUE PER LINE", text: $environment,
        prompt: "OPTION=value")
      Text(
        "The command inherits your login environment. PATH, HOME, USER and Soyeht profile variables cannot be overridden."
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.small))
      .foregroundStyle(MacTheme.muted)
      if let failure {
        Text(failure)
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .semibold))
          .foregroundStyle(MacTheme.danger)
      }
      HStack {
        Spacer()
        Button(original == nil ? "Add harness" : "Save changes") { save() }
          .buttonStyle(NeoPillButtonStyle())
          .disabled(!valid || submitting)
      }
    }
    .padding(22)
    .macSheetSurface(.harnessEditor)
    .foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onExitCommand { isPresented = false }
  }

  private var parsedEnvironment: [HarnessEnvironmentVariable]? {
    let lines = environment.split(whereSeparator: \.isNewline).map(String.init)
    guard !lines.isEmpty else { return [] }
    var values: [HarnessEnvironmentVariable] = []
    for line in lines {
      let pieces = line.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
      guard pieces.count == 2, !pieces[0].isEmpty else { return nil }
      values.append(HarnessEnvironmentVariable(key: String(pieces[0]), value: String(pieces[1])))
    }
    return values
  }

  private var valid: Bool {
    !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && !command.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && !version.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
      && parsedEnvironment != nil
  }

  private func field(_ label: String, text: Binding<String>, prompt: String) -> some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      Text(label).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.secondary)
      TextField(prompt, text: text).textFieldStyle(.plain).padding(MacTheme.Space.x10)
        .neoWell(radius: MacTheme.Radius.compact)
    }
  }

  private func editor(_ label: String, text: Binding<String>, prompt: String) -> some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      Text(label).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.secondary)
      TextEditor(text: text).font(MacTheme.mono(MacTheme.TypeScale.small)).scrollContentBackground(
        .hidden
      )
      .overlay(alignment: .topLeading) {
        if text.wrappedValue.isEmpty {
          Text(prompt).font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
            .padding(.horizontal, MacTheme.Space.x4).padding(.vertical, MacTheme.Space.x8)
            .allowsHitTesting(false)
        }
      }
      .padding(MacTheme.Space.x6).frame(height: 72).neoWell(radius: MacTheme.Radius.compact)
    }
  }

  private func save() {
    guard let environment = parsedEnvironment else { return }
    submitting = true
    failure = nil
    let launch = HarnessLaunch(
      id: original?.id ?? "custom.\(UUID().uuidString.lowercased())",
      name: name.trimmingCharacters(in: .whitespacesAndNewlines),
      command: command.trimmingCharacters(in: .whitespacesAndNewlines),
      args: arguments.split(whereSeparator: \.isNewline).map(String.init),
      version: version.trimmingCharacters(in: .whitespacesAndNewlines),
      environment: environment.isEmpty ? nil : environment)
    Task {
      do {
        if original == nil {
          _ = try await client.createCustomHarness(launch)
        } else {
          _ = try await client.updateCustomHarness(launch)
        }
        isPresented = false
      } catch {
        failure = error.localizedDescription
        submitting = false
      }
    }
  }
}

private struct RawTerminalPreferenceRow: View {
  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text("Default shell").font(MacTheme.chrome(14, weight: .bold))
        Text("\(shell) · JetBrains Mono 13")
          .font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .medium))
          .foregroundStyle(MacTheme.muted)
      }
      .frame(width: 200, alignment: .leading)
      PreferenceChip(icon: "waveform.path.ecg", text: "Detect agent state heuristically")
      Spacer()
      Text("Chrome font: Nunito")
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
      PreferenceToggle(isOn: true)
    }
    .preferenceRowSurface()
  }

  private var shell: String { ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh" }
}

private struct PreferenceChip: View {
  let icon: String
  let text: String

  var body: some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: 10, weight: .semibold))
      Text(text)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .lineLimit(1)
      Image(systemName: "chevron.down")
        .font(.system(size: 8, weight: .bold))
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
