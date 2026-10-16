import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct NewPaneSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  @Binding var mode: PaneMode
  let onCancel: () -> Void
  let onCreated: (String, PaneLayoutKind) -> Void
  @State private var directory: URL?
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var permissionMode: String?
  @State private var showsAgentPicker = false
  @State private var submitting = false
  @State private var prompt = ""
  @FocusState private var promptFocused: Bool

  private var selectedHarness: HarnessInfo? { client.harness(id: harness) }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? { client.effortOption(harness: harness) }

  var body: some View {
    VStack(spacing: MacTheme.Space.x8) {
      HStack(spacing: MacTheme.Space.x7) {
        Circle().fill(MacTheme.muted).frame(width: 7, height: 7)
        Text("new pane").font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
        Spacer()
        Image(systemName: "rectangle.split.2x1").accessibilityHidden(true)
        Image(systemName: "rectangle.split.1x2").accessibilityHidden(true)
        Button(action: onCancel) {
          Image(systemName: "xmark")
        }
        .help("Close new pane")
      }
      .font(MacTheme.ui(MacTheme.TypeScale.control)).foregroundStyle(MacTheme.secondary)
      .padding(.horizontal, MacTheme.Space.x12)
      .frame(height: PaneLayoutContentMetrics.newPaneHeaderHeight)
      .background(MacTheme.well, in: Capsule())

      ScrollView(.vertical, showsIndicators: true) {
        VStack(spacing: PaneLayoutContentMetrics.newPaneSectionSpacing) {
          Text("What do you want to do?")
            .font(MacTheme.ui(MacTheme.TypeScale.heading, weight: .bold))
            .frame(height: PaneLayoutContentMetrics.newPaneHeadingHeight)
          VStack(spacing: 0) {
            if mode == .agent {
              ZStack(alignment: .topLeading) {
                TextField("", text: $prompt, axis: .vertical)
                  .textFieldStyle(.plain)
                  .lineLimit(1...4)
                  .font(MacTheme.ui(MacTheme.TypeScale.body))
                  .foregroundStyle(MacTheme.ink)
                  .focused($promptFocused)
                  .padding(.trailing, PaneLayoutContentMetrics.newPanePromptTrailingInset)
                  .accessibilityLabel("Ask anything")
                  .accessibilityHint("Type the first message for the new agent")
                if prompt.isEmpty {
                  Text("Ask anything. Choose a directory and an agent below.")
                    .font(MacTheme.ui(MacTheme.TypeScale.body))
                    .foregroundStyle(MacTheme.muted)
                    .allowsHitTesting(false)
                }
                HStack {
                  Spacer()
                  Button("⌘⇧C") { promptFocused = true }
                    .buttonStyle(.plain)
                    .keyboardShortcut("c", modifiers: [.command, .shift])
                    .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
                    .foregroundStyle(MacTheme.muted)
                    .accessibilityLabel("Focus new agent message")
                }
              }
              .contentShape(Rectangle())
              .onTapGesture { promptFocused = true }
            } else {
              HStack(alignment: .top) {
                Text("Open a terminal in the directory selected below.")
                  .font(MacTheme.ui(MacTheme.TypeScale.body))
                  .foregroundStyle(MacTheme.muted)
                Spacer()
              }
              Spacer()
            }
            HStack(spacing: MacTheme.Space.x8) {
              Button(action: chooseDirectory) {
                Image(systemName: "plus")
                  .font(.system(size: PaneLayoutContentMetrics.newPaneAddGlyphSize))
              }.help("Choose a directory")
              if mode == .agent {
                Button {
                  showsAgentPicker.toggle()
                } label: {
                  HStack(spacing: MacTheme.Space.x6) {
                    Image(systemName: "cpu")
                    Text(selectedAgentLabel)
                      .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
                    if !effort.isEmpty {
                      Text("·").foregroundStyle(MacTheme.muted)
                      Text(effort.capitalized)
                        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
                        .foregroundStyle(MacTheme.secondary)
                    }
                    Image(systemName: "chevron.down")
                      .font(.system(size: PaneLayoutContentMetrics.newPaneChevronGlyphSize))
                      .foregroundStyle(MacTheme.muted)
                  }
                }
                .popover(isPresented: $showsAgentPicker, arrowEdge: .bottom) { agentPicker }
              } else {
                Button(action: create) {
                  Text("Terminal")
                    .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
                }
                .buttonStyle(.plain)
                .disabled(directory == nil || submitting)
                .accessibilityHint("Open a terminal in the selected directory")
              }
              Spacer()
              Button(action: create) {
                PaneSendButtonLabel.mac
              }
              .buttonStyle(.plain)
              .disabled(
                directory == nil || (mode == .agent && selectedHarness?.available != true)
                  || prompt.utf8.count > ProtocolLimits.promptBytes || submitting
              )
              .keyboardShortcut(.return, modifiers: [.command, .shift])
            }
          }
          .padding(EdgeInsets(top: 14, leading: 16, bottom: 12, trailing: 16))
          .frame(maxWidth: .infinity)
          .frame(height: PaneLayoutContentMetrics.newPaneFormHeight)
          .paneCard(radius: PaneInteriorMetrics.composerRadius)

          metadata
            .frame(height: PaneLayoutContentMetrics.newPaneMetadataHeight)
          ViewThatFits(in: .horizontal) {
            modeSwitch(horizontal: true)
            modeSwitch(horizontal: false)
          }
          .frame(
            maxWidth: .infinity,
            minHeight: PaneLayoutContentMetrics.newPaneCompactFooterHeight,
            alignment: .topLeading)
        }
        .padding(.vertical, PaneLayoutContentMetrics.newPaneBodyVerticalInset)
        .frame(maxWidth: .infinity)
      }
      .scrollBounceBehavior(.basedOnSize)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
    .padding(PaneLayoutContentMetrics.paneCardInset)
    .paneShell()
    .onAppear {
      client.refreshHarnesses(rescan: true)
      selectFirstHarness()
      selectWorkspaceDirectory()
    }
    .onChange(of: HarnessPickerModel.availabilityKeys(client.harnesses)) { _, _ in
      selectFirstHarness()
    }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      permissionMode = nil
      if selectedHarness?.available == true { client.refreshCatalogue(harness: value) }
    }
  }

  @ViewBuilder private func modeSwitch(horizontal: Bool) -> some View {
    let content = Group {
      Text(mode == .agent ? "Prefer the command line?" : "Prefer a structured agent?")
        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
      Button(action: toggleMode) {
        Label(
          mode == .agent ? "Open a terminal" : "Open an agent",
          systemImage: mode == .agent ? "terminal" : "cpu")
      }
      .buttonStyle(.paneSecondary)
      Text("⌘⇧Enter")
        .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
    }
    if horizontal {
      HStack(spacing: MacTheme.Space.x10) { content }
    } else {
      VStack(alignment: .leading, spacing: MacTheme.Space.x8) { content }
    }
  }

  private func toggleMode() {
    mode = mode == .agent ? .terminal : .agent
    if mode == .agent {
      showsAgentPicker = true
      promptFocused = true
    }
  }

  private var metadata: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Button(action: chooseDirectory) {
        Label(
          directory?.lastPathComponent ?? "Project", systemImage: "folder"
        )
        .lineLimit(1)
      }
      .layoutPriority(1)
      Label("Local", systemImage: "desktopcomputer")
        .fixedSize()
      Label("main", systemImage: "arrow.triangle.branch")
        .fixedSize()
      Spacer()
      if mode == .agent {
        Menu {
          PermissionModeChoices(modes: permissionModes, selection: $permissionMode)
        } label: {
          Label(
            PermissionModeChoices.title(
              modes: permissionModes, selection: permissionMode,
              preferred: client.defaultPermissionMode(harness: harness)),
            systemImage: "checkmark.shield")
        }.menuStyle(.borderlessButton).fixedSize()
      }
    }
    .font(MacTheme.ui(MacTheme.TypeScale.control, weight: .semibold))
    .foregroundStyle(MacTheme.secondary)
    .frame(maxWidth: .infinity)
  }

  private var agentPicker: some View {
    AgentHarnessPicker(
      client: client, harness: $harness, model: $model, effort: $effort,
      catalogue: catalogue, effortOption: effortOption, excludingHarness: nil)
  }

  private var selectedAgentLabel: String {
    let modelName = catalogue?.models.first { $0.id == model }?.name
    return modelName ?? selectedHarness?.name ?? "Choose agent"
  }

  private var permissionModes: AgentPermissionModes? {
    catalogue.flatMap { AgentPermissionModes(options: $0.options) }
  }

  private func selectFirstHarness() {
    guard selectedHarness?.available != true,
      let first = HarnessPickerModel.defaultHarness(
        in: client.harnesses, preferredId: client.preferredHarnessId)
    else { return }
    harness = first.id
  }

  private func chooseDirectory() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.prompt = "Choose"
    panel.begin { response in
      if response == .OK { directory = panel.url }
    }
  }

  private func selectWorkspaceDirectory() {
    guard directory == nil,
      let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return }
    directory = URL(fileURLWithPath: workspace.path, isDirectory: true)
  }

  private func create() {
    guard let directory, !submitting else { return }
    let firstPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
    guard firstPrompt.utf8.count <= ProtocolLimits.promptBytes else { return }
    submitting = true
    Task { @MainActor in
      do {
        let paneId: String
        if mode == .terminal {
          paneId = try await client.createTerminalPane(directory: directory)
        } else {
          paneId = try await client.createAgentPaneNow(
            directory: directory, harness: harness, model: model.isEmpty ? nil : model,
            effort: effort.isEmpty ? nil : effort, permissionMode: permissionMode)
        }
        onCreated(paneId, PaneLayoutKind(mode: mode))
        if mode == .agent {
          if !firstPrompt.isEmpty {
            try await client.sendPromptNow(paneId, text: firstPrompt)
          }
        }
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}
