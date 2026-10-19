import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The pane the new-pane sheet can make: an agent, a terminal or a writing document.
private enum PhonePaneChoice: String, CaseIterable, Identifiable {
  case agent, terminal, document
  var id: String { rawValue }

  var title: String {
    switch self {
    case .agent: "Agent"
    case .terminal: "Terminal"
    case .document: "Document"
    }
  }

  init(kind: NewPaneKind) {
    switch kind {
    case .agent: self = .agent
    case .terminal: self = .terminal
    case .writing: self = .document
    }
  }
}

private struct PhoneAgentRoute: Hashable {}

/// Opens a new terminal, agent or document in a workspace of the paired Mac. It is the
/// iPhone and iPad form of the Mac's new-pane sheet, over the same `NewPaneForm`.
struct PhoneNewPaneSheet: View {
  let client: EngineClient
  /// The workspace the pane is for; its folder is where it starts. Nil starts from the
  /// workspace on screen.
  let workspaceId: String?
  let created: (String) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var form = NewPaneForm()
  @State private var section = WritingSection.texts
  @State private var route = NavigationPath()
  @State private var creating = false
  @State private var failure: String?

  private var choice: Binding<PhonePaneChoice> {
    Binding(
      get: { PhonePaneChoice(kind: form.kind) },
      set: { next in
        switch next {
        case .agent: form.kind = .agent
        case .terminal: form.kind = .terminal
        case .document: form.kind = .writing(section)
        }
      })
  }

  private var blocker: NewPaneBlocker? { form.blocker(harnesses: client.harnesses) }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[form.harness] }
  private var permissionModes: AgentPermissionModes? {
    catalogue.flatMap { AgentPermissionModes(options: $0.options) }
  }

  var body: some View {
    NavigationStack(path: $route) {
      Form {
        Section {
          Picker("Open", selection: choice) {
            ForEach(PhonePaneChoice.allCases) { Text($0.title).tag($0) }
          }
          .pickerStyle(.segmented)
          .listRowBackground(Color.clear)
          .accessibilityIdentifier("new-pane-kind")
        }
        Section("Folder on the Mac") { folderRow }
          .listRowBackground(PhonePalette.face)
        switch form.kind {
        case .agent: agentSections
        case .terminal: terminalSection
        case .writing: documentSections
        }
        if let failure {
          Section { Text(failure).foregroundStyle(PhonePalette.danger) }
            .listRowBackground(PhonePalette.face)
        } else if let blocker, form.kind != .agent || !form.harness.isEmpty {
          Section { Text(blocker.explanation).foregroundStyle(PhonePalette.secondary) }
            .listRowBackground(PhonePalette.face)
        }
      }
      .phoneCreatePage(
        title: "New pane",
        canCreate: form.canCreate(harnesses: client.harnesses, submitting: creating),
        identifier: "new-pane-create", create: create
      )
      .phoneFolderDestinations(client: client, selected: form.directory) { chosen in
        form.directory = chosen
        route = NavigationPath()
      }
      .navigationDestination(for: PhoneAgentRoute.self) { _ in
        PhoneAgentChoiceView(client: client, form: $form)
      }
    }
    .tint(PhonePalette.accent)
    .foregroundStyle(PhonePalette.ink)
    .onAppear(perform: prepare)
    .onChange(of: HarnessPickerModel.availabilityKeys(client.harnesses)) {
      form.keepOrChooseHarness(in: client.harnesses, preferredId: client.preferredHarnessId)
    }
    .onChange(of: section) {
      if case .writing = form.kind { form.kind = .writing(section) }
    }
    .onChange(of: form.harness) {
      if client.harness(id: form.harness)?.available == true {
        client.refreshCatalogue(harness: form.harness)
      }
    }
  }

  private var folderRow: some View {
    NavigationLink(value: PhoneFolderRoute.picker) {
      if form.directory.isEmpty {
        Label("Choose a folder", systemImage: "folder.badge.plus")
      } else {
        PhoneFolderLabel(
          name: WorkspaceFolderPolicy.name(of: form.directory), path: form.directory)
      }
    }
    .accessibilityIdentifier("new-pane-folder")
  }

  @ViewBuilder private var agentSections: some View {
    Section("Agent") {
      NavigationLink(value: PhoneAgentRoute()) {
        HStack {
          Label(
            form.agentLabel(harnesses: client.harnesses, catalogue: catalogue),
            systemImage: "cpu")
          if !form.effort.isEmpty {
            Text(form.effort.capitalized).foregroundStyle(PhonePalette.secondary)
          }
        }
      }
      .accessibilityIdentifier("new-pane-agent")
      Menu {
        PermissionModeChoices(modes: permissionModes, selection: $form.permissionMode)
      } label: {
        HStack {
          Label("Permissions", systemImage: "checkmark.shield")
          Spacer()
          Text(
            PermissionModeChoices.title(
              modes: permissionModes, selection: form.permissionMode,
              preferred: client.defaultPermissionMode(harness: form.harness))
          )
          .foregroundStyle(PhonePalette.secondary)
          Image(systemName: "chevron.up.chevron.down").foregroundStyle(PhonePalette.muted)
        }
      }
      .disabled(permissionModes == nil)
    }
    .listRowBackground(PhonePalette.face)
    Section("First message") {
      TextField("Ask anything. It is optional.", text: $form.prompt, axis: .vertical)
        .lineLimit(1...6)
        .accessibilityIdentifier("new-pane-prompt")
    }
    .listRowBackground(PhonePalette.face)
  }

  private var terminalSection: some View {
    Section {
      Text("Opens a shell on the Mac in this folder.").foregroundStyle(PhonePalette.secondary)
    }
    .listRowBackground(PhonePalette.face)
  }

  @ViewBuilder private var documentSections: some View {
    Section {
      Picker("Kind", selection: $section) {
        ForEach(WritingSection.allCases, id: \.self) { Text($0.singularTitle.capitalized).tag($0) }
      }
      .pickerStyle(.segmented)
      TextField("Title", text: $form.title)
        .accessibilityIdentifier("new-pane-title")
    } header: {
      Text("New \(section.singularTitle)")
    } footer: {
      Text("The document is created in the workspace's \(section.title.lowercased()) folder.")
    }
    .listRowBackground(PhonePalette.face)
  }

  private func prepare() {
    client.refreshHarnesses(rescan: true)
    form.keepOrChooseHarness(in: client.harnesses, preferredId: client.preferredHarnessId)
    guard form.directory.isEmpty else { return }
    let target =
      workspaceId ?? client.panes.first { $0.paneId == client.selectedPaneId }?.workspaceId
    form.directory =
      client.workspaces.first { $0.workspaceId == target }?.path
      ?? WorkspaceFolderPolicy.recent(workspaces: client.workspaces, panes: client.panes)
      .first?.path ?? ""
  }

  private func create() {
    let request = form
    guard request.canCreate(harnesses: client.harnesses, submitting: creating) else { return }
    creating = true
    failure = nil
    Task { @MainActor in
      defer { creating = false }
      do {
        let paneId = try await client.createPane(request)
        created(paneId)
        dismiss()
        if request.kind == .agent, !request.firstPrompt.isEmpty {
          do {
            try await client.sendPromptNow(paneId, text: request.firstPrompt)
          } catch {
            client.problem = "The agent opened, but its first message was not sent."
          }
        }
      } catch {
        failure = error.localizedDescription
      }
    }
  }
}
