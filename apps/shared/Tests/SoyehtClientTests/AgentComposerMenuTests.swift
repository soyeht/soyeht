#if os(macOS)
  import AppKit
  import SoyehtProtocol
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// VM1 crash on the model chip: AppKit raised an exception in
  /// `_NSCreateAttributedTitleForTitleAndSubtitle` when SwiftUI re-set the items of the open
  /// menu, and every composer menu item with a description carried a subtitle. These tests
  /// build each chip's menu through AppKit (`NSHostingMenu`, the menu the chip opens) for a
  /// Claude pane and a Codex pane whose choices all have descriptions, and require titles
  /// only: the descriptions become tooltips.
  private enum MenuFixture {
    static let claude = AgentSession(
      harness: "registry.claude-acp", version: "1", fingerprint: "f",
      options: [
        select(
          "model", "model", "opus",
          [("opus", "Opus 5 (1M)", "Most capable"), ("sonnet", "Sonnet 5", "Fast")]),
        select(
          "effort", "thought_level", "high",
          [("low", "low", "Quick"), ("high", "high", "Thorough"), ("max", "max", "Longest")]),
        select(
          "mode", "mode", "acceptEdits",
          [
            ("default", "Manual", "Asks before every edit"),
            ("acceptEdits", "Accept edits", "Edits files without asking"),
            ("bypassPermissions", "Bypass", "Approves everything"),
          ], kinds: ["bypassPermissions": "full_access"]),
      ],
      authMethods: [], queue: [], draft: false, role: .worker)

    static let codex = AgentSession(
      harness: "registry.codex-acp", version: "1", fingerprint: "f",
      options: [
        select(
          "model", "model", "gpt-6-astra",
          [("gpt-6-astra", "6 Astra", "Latest frontier model"), ("gpt-6-mini", "6 Mini", "Fast")]
        ),
        select(
          "reasoning_effort", "thought_level", "medium",
          [
            ("low", "low", "Fast"), ("medium", "medium", "Balanced"),
            ("xhigh", "extra high", "Deep"),
          ]
        ),
        select(
          "mode", "mode", "ask",
          [
            ("ask", "Ask for approval", "Asks before running commands"),
            ("auto", "Approve for me", "Auto-reviews requests"),
            ("full", "Full access", "Runs everything"),
          ], kinds: ["full": "full_access"]),
      ],
      authMethods: [], queue: [], draft: false, role: .worker)

    static func select(
      _ id: String, _ category: String, _ current: String,
      _ choices: [(String, String, String)], kinds: [String: String] = [:]
    ) -> ConfigOption {
      ConfigOption(
        id: id, name: id, category: category, type: .select, currentValue: .v0(current),
        options: choices.map {
          ConfigChoice(value: $0.0, name: $0.1, description: $0.2, kind: kinds[$0.0])
        })
    }

    static func controls(_ agent: AgentSession, catalogue: [ModelInfo] = [])
      -> AgentComposerControls
    {
      AgentComposerControls(
        harnessName: "Agent", agent: agent, state: .idle, usage: nil, hasDraft: false,
        connected: true, catalogue: catalogue)
    }
  }

  /// The AppKit menu a chip opens, with its items as SwiftUI builds them.
  @available(macOS 15, *)
  @MainActor private func appKitMenu(_ content: some View) -> NSMenu {
    let menu = NSHostingMenu(rootView: content)
    menu.update()
    return menu
  }

  @MainActor @Test(arguments: ["claude", "codex"])
  func composerChipMenusOpenWithTitlesOnly(harness: String) throws {
    guard #available(macOS 15, *) else { return }
    let agent = harness == "claude" ? MenuFixture.claude : MenuFixture.codex
    let controls = MenuFixture.controls(agent)
    let model = try #require(controls.model)
    let effort = try #require(controls.effort)
    let modes = try #require(controls.permission)
    let menus: [(String, NSMenu, [String])] = [
      (
        "model", appKitMenu(AgentConfigMenuItems(menu: model, select: { _ in })),
        model.choices.map(\.name)
      ),
      (
        "effort", appKitMenu(AgentConfigMenuItems(menu: effort, select: { _ in })),
        effort.choices.map(\.name)
      ),
      (
        "permission", appKitMenu(AgentPermissionModeItems(modes: modes, select: { _ in })),
        modes.modes.map(\.name)
      ),
    ]
    for (name, menu, titles) in menus {
      let items = menu.items.filter { !$0.isSeparatorItem }
      #expect(items.map(\.title) == titles, "\(harness) \(name)")
      for item in items {
        #expect(item.subtitle == nil, "\(harness) \(name): \(item.title) has a subtitle")
      }
      // The current choice is checked and disabled; the others can be chosen.
      #expect(items.filter { !$0.isEnabled }.count == 1, "\(harness) \(name)")
      // Refreshing the menu again, as SwiftUI does while it is open, keeps it intact.
      menu.update()
      #expect(menu.items.filter { !$0.isSeparatorItem }.count == titles.count)
    }
    // The descriptions stay reachable as tooltips.
    let tooltips = menus[0].1.items.compactMap(\.toolTip)
    #expect(tooltips.count == model.choices.count, "\(harness) model tooltips")
  }

  @MainActor @Test func permissionChoicesBeforeAPaneExistsHaveNoSubtitles() throws {
    guard #available(macOS 15, *) else { return }
    let modes = try #require(MenuFixture.controls(MenuFixture.claude).permission)
    let menu = appKitMenu(PermissionModeChoices(modes: modes, selection: .constant(nil)))
    #expect(menu.items.contains { $0.title == "Harness default" })
    #expect(menu.items.allSatisfy { $0.subtitle == nil })
  }

  @Test func modelChipNamesTheModelAsTheCatalogueDoes() throws {
    let catalogue = [
      ModelInfo(id: "gpt-6-astra", name: "GPT-6 Astra"),
      ModelInfo(id: "gpt-6-mini", name: "GPT-6 Mini"),
    ]
    let model = try #require(MenuFixture.controls(MenuFixture.codex, catalogue: catalogue).model)
    #expect(model.label == "GPT-6 Astra")
    #expect(model.choices.map(\.name) == ["GPT-6 Astra", "GPT-6 Mini"])
    // Without the catalogue the chip keeps the name the harness announced.
    #expect(try #require(MenuFixture.controls(MenuFixture.codex).model).label == "6 Astra")
  }

  @Test func contextRingShowsForEverySessionAndFillsOnceUsageArrives() {
    let empty = MenuFixture.controls(MenuFixture.codex)
    #expect(empty.showsContext)
    #expect(empty.contextFraction == nil)
    #expect(empty.contextPercentage == nil)
    let none = AgentComposerControls(
      harnessName: "Agent", agent: nil, state: nil, usage: nil, hasDraft: false,
      connected: true)
    #expect(!none.showsContext)
  }
#endif
