import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct NewTaskSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  @Binding var isPresented: Bool
  @State private var title = ""
  @State private var description = ""
  @State private var mustNotChange = ""
  @State private var howToVerify = ""
  @State private var priority = TaskPriority.medium
  @State private var createWorktree = true
  @State private var permissionMode: String?
  @State private var workspaceDefault = true
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var dependencies: Set<String> = []
  @State private var showHowToVerify = false
  @State private var submitting = false

  private var valid: Bool {
    missingRequiredFields.isEmpty && (workspaceDefault || !harness.isEmpty)
  }
  private var missingRequiredFields: [String] {
    TaskPresentation.missingRequiredFields(
      title: title, description: description, mustNotChange: mustNotChange)
  }
  private var actionHint: String {
    TaskPresentation.actionHint(
      missing: missingRequiredFields, workspaceDefault: workspaceDefault, harness: harness,
      shortcuts: "⌘↩ create · ⇧⌘↩ create & dispatch")
  }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var permissionModes: AgentPermissionModes? {
    catalogue.flatMap { AgentPermissionModes(options: $0.options) }
  }
  private var effortOption: ConfigOption? {
    catalogue?.options.first(where: \.isEffort)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      HStack(alignment: .top, spacing: MacTheme.Space.x8) {
        VStack(alignment: .leading, spacing: 3) {
          Text("New task").font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
          Text("It enters this workspace list. An agent can claim it, or you can dispatch it.")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Button {
          isPresented = false
        } label: {
          Image(systemName: "xmark").font(.system(size: MacTheme.TypeScale.label, weight: .medium))
            .foregroundStyle(MacTheme.muted)
        }.buttonStyle(.plain)
      }
      TaskField(title: "TITLE · REQUIRED", text: $title)
      TaskEditor(
        title: "DESCRIPTION · REQUIRED", text: $description, showsAccessories: true,
        accessoryAction: { showHowToVerify = true }
      )
      .popover(isPresented: $showHowToVerify, arrowEdge: .bottom) { verificationEditor }
      TaskEditor(title: "MUST NOT CHANGE · REQUIRED", text: $mustNotChange, showsAccessories: true)
      runnerOptions
      taskOptions
      actions
    }
    .padding(EdgeInsets(top: 18, leading: 22, bottom: 18, trailing: 22))
    .frame(width: DesignTokens.Layout.newTaskSheetWidth).foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onAppear { client.refreshHarnesses() }
    .onExitCommand { isPresented = false }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      permissionMode = nil
      if !value.isEmpty { client.refreshCatalogue(harness: value) }
    }
  }

  private var runnerOptions: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      DialogFieldLabel(title: "WHO WILL RUN")
      HStack(spacing: MacTheme.Space.x10) {
        runnerMenu
        Spacer()
        WorkspaceDefaultToggle(isOn: $workspaceDefault)
      }.padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
    }
  }

  private var taskOptions: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        ForEach(TaskPriority.all, id: \.rawValue) { value in
          Button(value.title) { priority = value }
        }
      } label: {
        DialogOptionLabel(title: priority.title.lowercased(), icon: "flag")
      }
      Menu {
        Button(createWorktree ? "Use the workspace directly" : "Create a task worktree") {
          createWorktree.toggle()
        }
      } label: {
        DialogOptionLabel(
          title: createWorktree ? "worktree" : "workspace", icon: "arrow.triangle.branch")
      }
      Menu {
        PermissionModeChoices(modes: permissionModes, selection: $permissionMode)
      } label: {
        DialogOptionLabel(
          title: PermissionModeChoices.title(
            modes: permissionModes, selection: permissionMode,
            preferred: workspaceDefault ? nil : client.defaultPermissionMode(harness: harness)
          ).lowercased(),
          icon: "checkmark.shield")
      }
      .disabled(workspaceDefault)
      Menu {
        ForEach(client.tasks(in: workspaceId), id: \.taskId) { task in
          Button {
            toggleDependency(task.taskId)
          } label: {
            Label(
              "\(task.taskId) · \(task.title)",
              systemImage: dependencies.contains(task.taskId) ? "checkmark" : "circle")
          }
        }
      } label: {
        DialogOptionLabel(title: dependencyTitle, icon: "link")
      }
    }
    .menuIndicator(.hidden).buttonStyle(.plain).fixedSize(horizontal: false, vertical: true)
  }

  private var runnerMenu: some View {
    Menu {
      Button("Workspace default") { workspaceDefault = true }
      Section("Agent") {
        ForEach(client.harnesses.filter(\.available), id: \.id) { value in
          Button(value.name) {
            workspaceDefault = false
            harness = value.id
          }
        }
      }
      if !workspaceDefault, !harness.isEmpty {
        Section("Model") {
          Button("Agent default") { model = "" }
          ForEach(catalogue?.models ?? [], id: \.id) { value in
            Button(value.name) { model = value.id }
          }
        }
        Section("Effort") {
          Button("Agent default") { effort = "" }
          ForEach(effortOption?.options ?? [], id: \.value) { value in
            Button(value.name) { effort = value.value }
          }
        }
      }
    } label: {
      HStack(spacing: MacTheme.Space.x6) {
        Image(systemName: "cpu").font(.system(size: MacTheme.TypeScale.label, weight: .medium))
        Text(runnerTitle).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          .lineLimit(1)
        Image(systemName: "chevron.down").font(
          .system(size: MacTheme.TypeScale.control, weight: .medium)
        )
        .foregroundStyle(MacTheme.muted)
      }
      .foregroundStyle(MacTheme.ink)
      .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x6)
      .neoRaised(radius: MacTheme.Radius.pill, elevation: .taskCard)
    }.menuIndicator(.hidden).buttonStyle(.plain)
  }

  private var runnerTitle: String {
    TaskPresentation.runnerTitle(
      workspaceDefault: workspaceDefault,
      harnessName: client.harnesses.first { $0.id == harness }?.name,
      modelName: catalogue?.models.first { $0.id == model }?.name,
      effortName: effortOption?.options.first { $0.value == effort }?.name)
  }

  private var dependencyTitle: String {
    TaskPresentation.dependencyTitle(dependencies)
  }

  private var verificationEditor: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text("HOW TO VERIFY · OPTIONAL").font(
        MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold)
      ).tracking(1).foregroundStyle(MacTheme.muted)
      TextEditor(text: $howToVerify).font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .scrollContentBackground(.hidden).padding(MacTheme.Space.x10).frame(height: 112)
        .neoWell(radius: MacTheme.Radius.control)
    }.padding(MacTheme.Space.x18).frame(width: 360).background(MacTheme.face)
  }

  private var actions: some View {
    HStack(spacing: MacTheme.Space.x10) {
      Text(actionHint)
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
        .foregroundStyle(valid ? MacTheme.muted : MacTheme.warning)
      Spacer()
      Button("Create in list") { create(dispatch: false) }
        .buttonStyle(NeoPillButtonStyle(elevation: .action)).disabled(!valid || submitting)
        .accessibilityHint(valid ? "Create this task" : actionHint)
        .keyboardShortcut(.return, modifiers: [.command])
      Button {
        create(dispatch: true)
      } label: {
        Label("Create and dispatch", systemImage: "paperplane")
      }
      .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(!valid || submitting)
      .accessibilityHint(valid ? "Create and dispatch this task" : actionHint)
      .keyboardShortcut(.return, modifiers: [.command, .shift])
    }.padding(.top, MacTheme.Space.x6)
  }

  private func toggleDependency(_ taskId: String) {
    if dependencies.contains(taskId) {
      dependencies.remove(taskId)
    } else {
      dependencies.insert(taskId)
    }
  }

  private func create(dispatch: Bool) {
    guard valid, !submitting else { return }
    submitting = true
    let executor = TaskExecutor(
      workspaceDefault: workspaceDefault,
      harness: workspaceDefault ? nil : harness,
      model: model.nilIfEmpty, effort: effort.nilIfEmpty,
      permissionMode: workspaceDefault ? nil : permissionMode)
    Task { @MainActor in
      do {
        _ = try await client.createTask(
          workspaceId: workspaceId, title: title, description: description,
          howToVerify: howToVerify.nilIfEmpty, mustNotChange: mustNotChange,
          executor: executor, priority: priority, createWorktree: createWorktree,
          dependencies: dependencies.sorted(), dispatch: dispatch)
        isPresented = false
      } catch {
        submitting = false
        client.problem = error.localizedDescription
      }
    }
  }
}

extension String {
  fileprivate var nilIfEmpty: String? {
    let value = trimmingCharacters(in: .whitespacesAndNewlines)
    return value.isEmpty ? nil : value
  }
}
