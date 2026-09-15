import SoyehtClient
import SoyehtProtocol
import SwiftUI

private enum PreferencesSection {
  case appearance
  case harnesses
  case usage
  case editor
  case links
}

struct PreferencesPanel: View {
  @Bindable var client: EngineClient
  @State private var section: PreferencesSection
  @State private var selectedUsageProvider: UsageProvider?

  init(client: EngineClient, initialUsageProvider: UsageProvider? = nil) {
    self.client = client
    _section = State(initialValue: initialUsageProvider == nil ? .harnesses : .usage)
    _selectedUsageProvider = State(initialValue: initialUsageProvider)
  }

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      PreferencesSidebar(client: client, selection: $section)
      switch section {
      case .appearance: ThemePreferencesPanel()
      case .harnesses: PreferencesHarnesses(client: client)
      case .usage:
        UsageSettingsDetail(client: client, selectedProvider: $selectedUsageProvider)
      case .editor: EditorInputPreferencesPanel()
      case .links: WebLinkPreferencesPanel()
      }
    }
    .padding(MacTheme.Space.x10)
    .frame(width: 1080, height: 700)
    .foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onAppear { refreshCatalogues() }
    .onChange(of: client.harnesses.map(\.id)) { _, _ in refreshCatalogues() }
  }

  private func refreshCatalogues() {
    client.refreshHarnesses()
    for harness in client.harnesses where harness.available {
      client.refreshCatalogue(harness: harness.id)
    }
  }
}

private struct PreferencesSidebar: View {
  @Bindable var client: EngineClient
  @Binding var selection: PreferencesSection

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
      Text("Preferences")
        .font(MacTheme.chrome(16, weight: .bold))
        .padding(.horizontal, MacTheme.Space.x4)
      Color.clear.frame(height: MacTheme.Space.x8)
      sidebarRow("General", icon: "gearshape")
      sidebarButton("Appearance", icon: "paintpalette", section: .appearance)
      sidebarButton("Harnesses", icon: "cpu", section: .harnesses)
      sidebarButton("Usage", icon: "gauge.with.dots.needle.50percent", section: .usage)
      sidebarRow("Engine", icon: "memorychip")
      sidebarRow("Phone", icon: "iphone")
      sidebarRow("Tasks & loops", icon: "list.bullet.clipboard")
      sidebarButton("Links", icon: "link", section: .links)
      sidebarButton("Shortcuts", icon: "keyboard", section: .editor)
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

  private func sidebarRow(_ title: String, icon: String, selected: Bool = false) -> some View {
    HStack(spacing: MacTheme.Space.x10) {
      Image(systemName: icon)
        .font(.system(size: 13, weight: .semibold))
        .frame(width: 15, height: 15)
        .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
      Text(title)
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

  private func sidebarButton(
    _ title: String, icon: String, section: PreferencesSection
  ) -> some View {
    Button {
      selection = section
    } label: {
      sidebarRow(title, icon: icon, selected: selection == section)
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

private struct WebLinkPreferencesPanel: View {
  @Environment(WebLinkPreferences.self) private var preferences

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      Text("Web links").font(MacTheme.chrome(18, weight: .bold))
      Text("Local links open in a web pane. Choose whether other web links do too.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
      Button {
        preferences.setOpenExternalLinksInSoyeht(!preferences.openExternalLinksInSoyeht)
      } label: {
        HStack(spacing: MacTheme.Space.x14) {
          VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
            Text("Open external links in Soyeht")
              .font(MacTheme.chrome(14, weight: .bold))
            Text("When off, external links use the system browser")
              .font(MacTheme.chrome(MacTheme.TypeScale.control))
              .foregroundStyle(MacTheme.secondary)
          }
          Spacer()
          PreferenceToggle(isOn: preferences.openExternalLinksInSoyeht)
        }
        .padding(.horizontal, MacTheme.Space.x16).frame(height: 64)
        .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
      }
      .buttonStyle(.plain)
      Text("localhost and loopback links always open in Soyeht.")
        .font(MacTheme.chrome(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
      Spacer()
    }
    .padding(.vertical, MacTheme.Space.x16).padding(.horizontal, 22)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}

private struct EditorInputPreferencesPanel: View {
  @Environment(EditorInputPreferences.self) private var preferences

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      Text("Editor shortcuts").font(MacTheme.chrome(18, weight: .bold))
      Text("Choose how the editor interprets unmodified keys. This setting applies only to you.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      Button {
        preferences.setVimEnabled(!preferences.vimEnabled)
      } label: {
        HStack(spacing: MacTheme.Space.x14) {
          VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
            Text("Vim mode").font(MacTheme.chrome(14, weight: .bold))
            Text("Normal, insert and visual modes with bounded Ex commands")
              .font(MacTheme.chrome(MacTheme.TypeScale.control))
              .foregroundStyle(MacTheme.secondary)
          }
          Spacer()
          PreferenceToggle(isOn: preferences.vimEnabled)
        }
        .padding(.horizontal, MacTheme.Space.x16).frame(height: 64)
        .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
      }
      .buttonStyle(.plain)
      Text("Off restores the standard editor bindings immediately.")
        .font(MacTheme.chrome(MacTheme.TypeScale.small))
        .foregroundStyle(MacTheme.muted)
      Spacer()
    }
    .padding(.vertical, MacTheme.Space.x16).padding(.horizontal, 22)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
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
              isDefault: harness.id == client.harnesses.first(where: \.available)?.id,
              customLaunch: client.customHarnesses.first { $0.id == harness.id },
              edit: edit, remove: { pendingRemoval = $0 })
          }
          if client.harnesses.isEmpty {
            EmptyHarnessRow(connected: client.connected)
          }
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
  let edit: (HarnessLaunch) -> Void
  let remove: (HarnessLaunch) -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      identity
      HStack(spacing: MacTheme.Space.x8) {
        PreferenceChip(icon: "cpu", text: modelLabel)
        PreferenceChip(icon: "gauge.with.dots.needle.50percent", text: effortLabel)
        PreferenceChip(icon: "checkmark.shield", text: "Ask every time")
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
    .padding(.horizontal, MacTheme.Space.x16)
    .frame(height: 58)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
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
    HStack(spacing: MacTheme.Space.x6) {
      Circle().fill(statusColor).frame(width: 7, height: 7)
      Text(statusLabel)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
    }
    .frame(width: 118, alignment: .leading)
  }

  private var versionLabel: String {
    guard let version = harness.version, !version.isEmpty else {
      return "Version unavailable · ACP"
    }
    return "\(version) · ACP"
  }

  private var modelLabel: String {
    if let option = options.first(where: { $0.category == "model" }),
      case .v0(let value) = option.currentValue
    {
      return option.options.first(where: { $0.value == value })?.name ?? value
    }
    return catalogue?.models.first?.name ?? "Agent default"
  }

  private var effortLabel: String {
    guard
      let option = options.first(where: {
        $0.category == "thought_level" || $0.id == "effort"
      }), case .v0(let value) = option.currentValue
    else { return "Default effort" }
    return option.options.first(where: { $0.value == value })?.name ?? value.capitalized
  }

  private var options: [ConfigOption] { catalogue?.options ?? harness.options }

  private var statusLabel: String {
    switch harness.setupState {
    case .ready: return "ready"
    case .not_installed: return "not installed"
    case .sign_in: return "sign in"
    case .check_setup: return "check setup"
    case nil:
      if !harness.available { return "not installed" }
      if harness.capabilities.contains(where: { $0.result == .unauthenticated }) {
        return "sign in"
      }
      if harness.capabilities.contains(where: { $0.result == .inconclusive }) {
        return "check setup"
      }
      return "ready"
    }
  }

  private var statusColor: Color {
    if !harness.available { return MacTheme.muted }
    return statusLabel == "ready" ? MacTheme.success : MacTheme.warning
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
    .frame(width: 560)
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

private struct EmptyHarnessRow: View {
  let connected: Bool

  var body: some View {
    HStack(spacing: MacTheme.Space.x10) {
      ProgressView().controlSize(.small)
      Text(connected ? "Loading harnesses…" : "Waiting for the engine…")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
      Spacer()
    }
    .padding(.horizontal, MacTheme.Space.x16)
    .frame(height: 58)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
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
    .padding(.horizontal, MacTheme.Space.x16)
    .frame(height: 58)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
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

private struct PreferenceToggle: View {
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

private struct PreferencePreviewAction: View {
  let icon: String
  let title: String

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: icon).font(.system(size: 10, weight: .bold))
      Text(title).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
    }
    .foregroundStyle(MacTheme.onAccent)
    .padding(.horizontal, MacTheme.Space.x14)
    .frame(height: 28)
    .neoAccent(radius: MacTheme.Radius.pill)
    .accessibilityHidden(true)
  }
}
