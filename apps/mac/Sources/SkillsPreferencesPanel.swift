import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// Preferences > Skills: the user's Soyeht skills, which every harness loads
/// (ADR 0075). Kept simple: list, open, create and delete.
struct SkillsPreferencesPanel: View {
  @Bindable var client: EngineClient
  @State private var drafting = false
  @State private var name = ""
  @State private var summary = ""
  @State private var instructions = ""
  @State private var failure: String?
  @State private var pendingDelete: Skill?

  private var listing: SkillsListResult? { client.slashSources.skills[""] }

  var body: some View {
    PreferencePage(
      title: "Skills",
      summary:
        "Reusable instructions every agent in Soyeht can load: Claude Code and Codex natively, "
        + "other harnesses through the soyeht MCP server. Ask any agent to \u{201C}create a skill "
        + "that\u{2026}\u{201D}, or add one here."
    ) {
      HStack(spacing: MacTheme.Space.x10) {
        Text(listing?.userDirectory ?? "")
          .font(MacTheme.chrome(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
          .lineLimit(1).truncationMode(.middle).textSelection(.enabled)
        Spacer()
        Button("Open Folder") { open(listing?.userDirectory) }.disabled(listing == nil)
        Button("New Skill") { drafting = true }.disabled(drafting)
      }
      if drafting { draft }
      if let failure {
        Text(failure).font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.danger)
      }
      ScrollView {
        VStack(spacing: MacTheme.Space.x8) {
          ForEach(listing?.items ?? [], id: \.path) { skill in row(skill) }
        }
      }
    }
    .task { await run { try await client.loadSkills(workspaceId: nil) } }
    .confirmationDialog(
      "Delete the skill \u{201C}\(pendingDelete?.name ?? "")\u{201D} and its folder?",
      isPresented: Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
    ) {
      Button("Delete", role: .destructive) {
        if let skill = pendingDelete { Task { await run { try await client.deleteSkill(skill) } } }
      }
    }
  }

  private func row(_ skill: Skill) -> some View {
    HStack(alignment: .top, spacing: MacTheme.Space.x12) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        Text(skill.name).font(MacTheme.chrome(MacTheme.TypeScale.body, weight: .bold))
        if let problem = skill.problem {
          Text(problem).foregroundStyle(MacTheme.danger)
        } else {
          Text(skill.description ?? "").foregroundStyle(MacTheme.secondary)
        }
        if let tools = skill.allowedTools {
          Text("Pre-approves tools in Claude Code: \(tools)").foregroundStyle(MacTheme.warning)
        }
      }
      .font(MacTheme.chrome(MacTheme.TypeScale.control))
      Spacer()
      Button("Open") { open((skill.path as NSString).appendingPathComponent("SKILL.md")) }
      Button("Delete") { pendingDelete = skill }
    }
    .padding(MacTheme.Space.x12)
    .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
    .accessibilityIdentifier("skill-\(skill.name)")
  }

  private var draft: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
      TextField("name (lowercase-with-hyphens)", text: $name)
      TextField("When to use it: what it does and when an agent should load it", text: $summary)
      TextEditor(text: $instructions)
        .font(MacTheme.chrome(MacTheme.TypeScale.control))
        .frame(minHeight: PreferenceMetrics.rowHeight)
        .scrollContentBackground(.hidden)
        .background(MacTheme.well, in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
      HStack {
        Spacer()
        Button("Cancel") { reset() }
        Button("Create") {
          Task {
            await run {
              _ = try await client.createSkill(
                name: name, description: summary, instructions: instructions)
              reset()
            }
          }
        }
        .keyboardShortcut(.defaultAction)
        .disabled(name.isEmpty || summary.isEmpty)
      }
    }
    .textFieldStyle(.roundedBorder)
  }

  private func reset() {
    drafting = false
    name = ""
    summary = ""
    instructions = ""
  }

  private func run(_ action: () async throws -> Void) async {
    do {
      try await action()
      failure = nil
    } catch {
      failure = error.localizedDescription
    }
  }

  private func open(_ path: String?) {
    guard let path else { return }
    NSWorkspace.shared.open(URL(fileURLWithPath: path))
  }
}
