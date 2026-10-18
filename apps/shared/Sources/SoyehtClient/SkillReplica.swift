import Foundation
import SoyehtProtocol

/// What the composer's "/" list is built from: Soyeht skill listings by
/// workspace (the key `""` holds the user's skills alone) and the commands
/// each agent pane's harness advertised.
public struct SlashSources: Sendable {
  public internal(set) var skills: [String: SkillsListResult] = [:]
  public internal(set) var commands: [String: [AgentCommand]] = [:]
}

extension EngineClient {
  public func slashCatalog(paneId: String) -> SlashCatalog {
    let workspaceId = panes.first { $0.paneId == paneId }?.workspaceId ?? ""
    let listing = slashSources.skills[workspaceId] ?? slashSources.skills[""]
    return SlashCatalog(
      skills: listing?.items ?? [], commands: slashSources.commands[paneId] ?? [])
  }

  /// Loads the skills of the pane's workspace and the pane's commands.
  public func loadSlashSources(paneId: String) async {
    guard let workspaceId = panes.first(where: { $0.paneId == paneId })?.workspaceId else {
      return
    }
    do {
      try await loadSkills(workspaceId: workspaceId)
      try await loadCommands(paneId: paneId)
    } catch {
      if !absorbMissingPane(paneId, error) { problem = error.localizedDescription }
    }
  }

  /// The user's skills with `nil`, or the user's and the workspace's.
  /// Listing a workspace also makes the engine announce its changes.
  public func loadSkills(workspaceId: String?) async throws {
    let result = try await request(
      .skillsList, .encoded(SkillsListParams(workspaceId: workspaceId))
    ).decoded(SkillsListResult.self)
    slashSources.skills[workspaceId ?? ""] = result
  }

  func loadCommands(paneId: String) async throws {
    let result = try await request(
      .paneCommandsList, .encoded(PaneCommandsListParams(paneId: paneId))
    ).decoded(PaneCommandsListResult.self)
    slashSources.commands[paneId] = result.items
  }

  public func createSkill(
    name: String, description: String, instructions: String, workspaceId: String? = nil
  ) async throws -> Skill {
    let result = try await mutation(
      .skillsCreate,
      SkillsCreateParams(
        name: name, description: description, instructions: instructions,
        workspaceId: workspaceId, idempotencyKey: UUID().uuidString)
    ).decoded(SkillMutationResult.self)
    try await loadSkills(workspaceId: workspaceId)
    return result.skill
  }

  public func deleteSkill(_ skill: Skill) async throws {
    _ = try await mutation(
      .skillsDelete,
      SkillsDeleteParams(
        name: skill.name, workspaceId: skill.workspaceId, idempotencyKey: UUID().uuidString))
    try await loadSkills(workspaceId: skill.workspaceId)
  }

  func applySlashEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .skillsChanged:
      let event = try params.decoded(SkillsChangedEvent.self)
      // A user skill is part of every listing; a workspace skill of its own.
      let keys = slashSources.skills.keys.filter {
        event.scope == .user || $0 == event.workspaceId
      }
      for key in keys {
        perform { try await self.loadSkills(workspaceId: key.isEmpty ? nil : key) }
      }
    case .paneCommandsChanged:
      let event = try params.decoded(PaneCommandsChangedEvent.self)
      guard slashSources.commands[event.paneId] != nil else { return }
      perform { try await self.loadCommands(paneId: event.paneId) }
    default:
      break
    }
  }
}
