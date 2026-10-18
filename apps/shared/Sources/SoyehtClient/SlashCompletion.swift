import Foundation
import SoyehtProtocol

/// One entry of the composer's "/" list: a Soyeht skill or a command the
/// harness advertised. Choosing it puts `/name ` in the composer.
public struct SlashSuggestion: Equatable, Identifiable, Sendable {
  public enum Source: Equatable, Sendable {
    case skill(SkillScope)
    case command
  }

  public let name: String
  public let detail: String
  public let hint: String?
  public let source: Source
  public var id: String { name }
}

/// Soyeht skills merged with a pane's harness commands (ADR 0075).
///
/// A skill the harness also advertises appears once, as the skill: Claude
/// Code lists a delivered skill by its name and Codex as `$name`. Sending
/// keeps the harness's own invocation and falls back to pointing the agent
/// at the skill's `SKILL.md` when the harness does not know the skill.
public struct SlashCatalog: Sendable {
  public let skills: [Skill]
  public let commands: [AgentCommand]

  /// `skills` may hold broken folders and a user skill that a workspace
  /// skill of the same name hides; neither is offered.
  public init(skills: [Skill], commands: [AgentCommand]) {
    var usable: [Skill] = []
    for skill in skills where skill.problem == nil {
      if let index = usable.firstIndex(where: { $0.name == skill.name }) {
        if skill.scope == .workspace { usable[index] = skill }
      } else {
        usable.append(skill)
      }
    }
    self.skills = usable
    self.commands = commands
  }

  public var suggestions: [SlashSuggestion] {
    let names = Set(skills.map(\.name))
    let skillItems = skills.map {
      SlashSuggestion(
        name: $0.name, detail: $0.description ?? "", hint: nil, source: .skill($0.scope))
    }
    let commandItems = commands.filter { !names.contains(Self.skillName($0.name)) }.map {
      SlashSuggestion(name: $0.name, detail: $0.description, hint: $0.inputHint, source: .command)
    }
    return skillItems + commandItems
  }

  /// The word typed after a leading "/", while the composer holds nothing else.
  public static func query(in text: String) -> String? {
    guard text.hasPrefix("/") else { return nil }
    let query = text.dropFirst()
    return query.contains(where: \.isWhitespace) ? nil : String(query)
  }

  /// Suggestions for `query`: names that start with it, then names that
  /// contain it, then descriptions with a word that starts with it.
  public func matches(_ query: String) -> [SlashSuggestion] {
    let query = query.lowercased()
    guard !query.isEmpty else { return suggestions }
    let ranked = suggestions.compactMap { suggestion -> (Int, SlashSuggestion)? in
      let name = Self.skillName(suggestion.name).lowercased()
      if name.hasPrefix(query) { return (0, suggestion) }
      if name.contains(query) { return (1, suggestion) }
      let words = suggestion.detail.lowercased().split(whereSeparator: { !$0.isLetter })
      return words.contains(where: { $0.hasPrefix(query) }) ? (2, suggestion) : nil
    }
    return ranked.enumerated().sorted { ($0.element.0, $0.offset) < ($1.element.0, $1.offset) }
      .map(\.element.1)
  }

  /// What choosing `suggestion` leaves in the composer: `/name ` with the caret after the
  /// space (a character offset), so typing goes on with the arguments, not inside the name.
  public static func accept(_ suggestion: SlashSuggestion) -> (text: String, cursor: Int) {
    let text = "/\(suggestion.name) "
    return (text, text.count)
  }

  /// The prompt to send for composer text. Only `/skill arguments` for a
  /// skill the harness does not advertise under its own name changes.
  public func prompt(for text: String) -> String {
    guard text.hasPrefix("/") else { return text }
    let parts = text.dropFirst().split(maxSplits: 1, whereSeparator: \.isWhitespace)
    guard let first = parts.first else { return text }
    let name = String(first)
    let arguments = parts.count > 1 ? String(parts[1]) : ""
    guard !commands.contains(where: { $0.name == name }),
      let skill = skills.first(where: { $0.name == name })
    else { return text }
    if commands.contains(where: { $0.name == "$" + name }) {
      return arguments.isEmpty ? "$\(name)" : "$\(name) \(arguments)"
    }
    let manifest = (skill.path as NSString).appendingPathComponent("SKILL.md")
    let request = "Use the Soyeht skill \"\(name)\": read \(manifest) and follow it."
    return arguments.isEmpty ? request : request + "\n\n" + arguments
  }

  /// Codex advertises a skill as `$name`.
  private static func skillName(_ command: String) -> String {
    command.hasPrefix("$") ? String(command.dropFirst()) : command
  }
}
