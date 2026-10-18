import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func skill(
  _ name: String, _ scope: SkillScope = .user, detail: String = "Does things.",
  problem: String? = nil
) -> Skill {
  Skill(
    name: name, scope: scope, workspaceId: scope == .workspace ? "w" : nil,
    path: "/skills/\(scope.rawValue)/\(name)", description: problem == nil ? detail : nil,
    problem: problem)
}

private func command(_ name: String, _ detail: String = "Built in.") -> AgentCommand {
  AgentCommand(name: name, description: detail)
}

@Test func skillsComeFirstOnceAndHideTheHarnessCopyOfTheSameSkill() {
  let catalog = SlashCatalog(
    skills: [
      skill("review"), skill("review", .workspace, detail: "Workspace review."),
      skill("broken", problem: "The frontmatter has no description."), skill("notes"),
    ],
    commands: [command("compact"), command("review"), command("$notes"), command("$other")])

  #expect(catalog.suggestions.map(\.name) == ["review", "notes", "compact", "$other"])
  #expect(catalog.suggestions[0].detail == "Workspace review.")
  #expect(catalog.suggestions[0].source == .skill(.workspace))
  #expect(catalog.suggestions[2].source == .command)
}

@Test func typingFiltersByNamePrefixThenNameThenDescriptionWord() {
  let catalog = SlashCatalog(
    skills: [skill("pr-summary", detail: "Summarizes a pull request."), skill("deploy")],
    commands: [command("$review-branch", "Review the branch."), command("plan")])

  #expect(SlashCatalog.query(in: "/re") == "re")
  #expect(SlashCatalog.query(in: "/") == "")
  #expect(SlashCatalog.query(in: "/review now") == nil)
  #expect(SlashCatalog.query(in: "review") == nil)
  #expect(catalog.matches("").count == 4)
  #expect(catalog.matches("RE").map(\.name) == ["$review-branch", "pr-summary"])
  #expect(catalog.matches("pull").map(\.name) == ["pr-summary"])
  #expect(catalog.matches("zzz").isEmpty)
  let accepted = SlashCatalog.accept(catalog.matches("dep")[0])
  #expect(accepted.text == "/deploy ")
  #expect(accepted.cursor == accepted.text.count)
}

@Test func sendingUsesTheHarnessInvocationAndFallsBackToTheSkillFile() {
  let skills = [skill("notes"), skill("deploy", .workspace)]
  let claude = SlashCatalog(skills: skills, commands: [command("notes")])
  let codex = SlashCatalog(skills: skills, commands: [command("$notes")])
  let other = SlashCatalog(skills: skills, commands: [])

  #expect(claude.prompt(for: "/notes tomorrow") == "/notes tomorrow")
  #expect(codex.prompt(for: "/notes tomorrow") == "$notes tomorrow")
  #expect(codex.prompt(for: "/notes") == "$notes")
  #expect(
    other.prompt(for: "/deploy to staging")
      == "Use the Soyeht skill \"deploy\": read /skills/workspace/deploy/SKILL.md and follow it."
      + "\n\nto staging")
  #expect(other.prompt(for: "/unknown text") == "/unknown text")
  #expect(other.prompt(for: "plain text") == "plain text")
}

@Test func acceptingASkillPutsTheCaretAfterTheNameAndOneSpace() {
  let skill = SlashSuggestion(name: "qa-skill", detail: "", hint: nil, source: .skill(.user))
  let accepted = SlashCatalog.accept(skill)
  #expect(accepted.text == "/qa-skill ")
  #expect(accepted.cursor == 10)
  let index = accepted.text.index(accepted.text.startIndex, offsetBy: accepted.cursor)
  #expect(accepted.text[index...].isEmpty)
}
