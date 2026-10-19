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
  @State private var form = NewPaneForm()
  @State private var showsAgentPicker = false
  @State private var submitting = false
  @FocusState private var promptFocused: Bool

  /// The form with the kind the mode switch shows.
  private var current: NewPaneForm {
    var value = form
    value.kind = NewPaneKind(mode: mode)
    return value
  }
  private var canCreate: Bool {
    current.canCreate(harnesses: client.harnesses, submitting: submitting)
  }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[form.harness] }
  private var effortOption: ConfigOption? { client.effortOption(harness: form.harness) }

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
                TextField("", text: $form.prompt, axis: .vertical)
                  .textFieldStyle(.plain)
                  .lineLimit(1...4)
                  .font(MacTheme.ui(MacTheme.TypeScale.body))
                  .foregroundStyle(MacTheme.ink)
                  .focused($promptFocused)
                  .padding(.trailing, PaneLayoutContentMetrics.newPanePromptTrailingInset)
                  .accessibilityLabel("Ask anything")
                  .accessibilityHint("Type the first message for the new agent")
                if form.prompt.isEmpty {
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
                    Text(current.agentLabel(harnesses: client.harnesses, catalogue: catalogue))
                      .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
                    if !form.effort.isEmpty {
                      Text("·").foregroundStyle(MacTheme.muted)
                      Text(form.effort.capitalized)
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
                .disabled(!canCreate)
                .accessibilityHint("Open a terminal in the selected directory")
              }
              Spacer()
              Button(action: create) {
                PaneSendButtonLabel.mac
              }
              .buttonStyle(.plain)
              .disabled(!canCreate)
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
        .forceEnclosingOverlayScrollers()
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
    .onChange(of: form.harness) { _, value in
      if client.harness(id: value)?.available == true { client.refreshCatalogue(harness: value) }
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
          form.directory.isEmpty ? "Project" : WorkspaceFolderPolicy.name(of: form.directory),
          systemImage: "folder"
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
          PermissionModeChoices(modes: permissionModes, selection: $form.permissionMode)
        } label: {
          Label(
            PermissionModeChoices.title(
              modes: permissionModes, selection: form.permissionMode,
              preferred: client.defaultPermissionMode(harness: form.harness)),
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
      client: client,
      harness: Binding(get: { form.harness }, set: { form.chooseHarness($0) }),
      model: $form.model, effort: $form.effort,
      catalogue: catalogue, effortOption: effortOption, excludingHarness: nil)
  }

  private var permissionModes: AgentPermissionModes? {
    catalogue.flatMap { AgentPermissionModes(options: $0.options) }
  }

  private func selectFirstHarness() {
    form.keepOrChooseHarness(in: client.harnesses, preferredId: client.preferredHarnessId)
  }

  private func chooseDirectory() {
    let panel = NSOpenPanel()
    panel.canChooseDirectories = true
    panel.canChooseFiles = false
    panel.allowsMultipleSelection = false
    panel.prompt = "Choose"
    panel.begin { response in
      if response == .OK, let url = panel.url { form.directory = url.standardizedFileURL.path }
    }
  }

  private func selectWorkspaceDirectory() {
    guard form.directory.isEmpty,
      let workspace = client.workspaces.first(where: { $0.workspaceId == workspaceId })
    else { return }
    form.directory = workspace.path
  }

  private func create() {
    guard canCreate else { return }
    let request = current
    submitting = true
    Task { @MainActor in
      do {
        let paneId = try await client.createPane(request)
        onCreated(paneId, PaneLayoutKind(mode: mode))
        if request.kind == .agent, !request.firstPrompt.isEmpty {
          try await client.sendPromptNow(paneId, text: request.firstPrompt)
        }
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}
