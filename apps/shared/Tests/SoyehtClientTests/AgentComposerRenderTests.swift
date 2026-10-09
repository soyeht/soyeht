#if os(macOS)
  import AppKit
  import SoyehtProtocol
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The approved composer and pane header (`DCzvR` and the "Cabeçalho" ticker in frames
  /// `M0eXP8` Milk and `M7MPUP` Deep Harbor) render flat in the palette, with no white pixel.
  /// With `SOYEHT_DESIGN_SHOTS_DIR` set, each render is written there as a PNG.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func agentComposerAndHeaderRenderAsApproved(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let style = AgentConversationStyle.pane(SoyehtThemeColorSet(palette: theme.palette))
    let palette = theme.palette
    let shots: [(String, AnyView, CGSize)] = [
      (
        "composer-codex-running", AnyView(ComposerShot.host(.codexRunning, style)),
        ComposerShot.size
      ),
      ("composer-claude-idle", AnyView(ComposerShot.host(.claudeIdle, style)), ComposerShot.size),
      // An iPhone's compact width: the chips scroll sideways, ring, mic and send stay put.
      (
        "composer-codex-running-compact", AnyView(ComposerShot.host(.codexRunning, style)),
        ComposerShot.compactSize
      ),
      ("header-codex", AnyView(ComposerShot.header(.codexRunning, style)), ComposerShot.headerSize),
      ("header-claude", AnyView(ComposerShot.header(.claudeIdle, style)), ComposerShot.headerSize),
    ]
    for (name, view, size) in shots {
      let pixels = try PaneInteriorRendering.shot(name, view, size: size, theme: theme)
      if name.hasPrefix("composer") {
        #expect(
          PaneInteriorRendering.share(of: palette.canvas, in: pixels) > 0.5, "\(theme) \(name)")
        #expect(
          PaneInteriorRendering.share(of: palette.ink, in: pixels) > 0.008, "\(theme) \(name)")
      }
    }
  }

  /// VM1 regression: the "/" list covered the composer's mode chip. It floats above the card
  /// now: with the list open, every pixel of the card is the same as with no list, and the
  /// list draws in the room above the card. With `SOYEHT_DESIGN_SHOTS_DIR` set, the open list
  /// is written there as a PNG.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func slashListFloatsAboveTheComposerCard(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let style = AgentConversationStyle.pane(SoyehtThemeColorSet(palette: theme.palette))
    let commands = ["review", "compact", "init"].map {
      AgentCommand(name: $0, description: "Run /\($0)")
    }
    let open = ComposerShot.client(.codexRunning)
    open.slashSources.commands["pane"] = commands
    let size = CGSize(width: ComposerShot.size.width, height: 300)
    func render(_ client: EngineClient) throws -> NSBitmapImageRep {
      let view = ComposerShot.host(
        .codexRunning, style, client: client, prompt: "/", alignment: .bottom)
      return try #require(PaneInteriorRendering.bitmap(view, size: size))
    }
    let withList = try render(open)
    let withoutList = try render(ComposerShot.client(.codexRunning))
    // Without the list, the first pixel row that is not the plain background is the card's.
    let cardTop = try #require(PaneInteriorRendering.firstRowUnlikeTheFirst(withoutList))
    let rows = PaneInteriorRendering.rowsEqual(withList, withoutList)
    #expect(rows[cardTop...].allSatisfy { $0 }, "\(theme): the list covers the composer card")
    #expect(rows[..<cardTop].contains(false), "\(theme): the list is not drawn above the card")
    if let directory = ProcessInfo.processInfo.environment["SOYEHT_DESIGN_SHOTS_DIR"] {
      let url = URL(fileURLWithPath: directory)
        .appendingPathComponent("composer-slash-list-\(theme.rawValue).png")
      try withList.representation(using: .png, properties: [:])?.write(to: url)
    }
  }

  /// The two panes of the approved frames: Codex running and asking for approval, Claude idle.
  @MainActor enum ComposerShot {
    enum Scenario { case codexRunning, claudeIdle }

    /// The design's composer is 678 points wide inside a 16-point pane interior.
    static let size = CGSize(width: 710, height: 111)
    static let compactSize = CGSize(width: 375, height: 111)
    /// The approved pane interior pads its content by 16 points.
    static let interiorPadding: CGFloat = 16
    static let headerSize = CGSize(width: 710, height: 52)

    static func client(_ scenario: Scenario) -> EngineClient {
      let client = EngineClient(socket: "/tmp/sn-composer-shot/engine.sock")
      let codex = scenario == .codexRunning
      let options: [ConfigOption] =
        codex
        ? [
          select("model", "model", "gpt-6-astra", ["gpt-6-astra"]),
          select("reasoning_effort", "thought_level", "medium", ["low", "medium", "high"]),
          select("mode", "mode", "ask", ["ask"], names: ["Ask for approval"]),
        ]
        : [
          select("model", "model", "opus", ["opus"], names: ["Opus 5 (1M)"]),
          select("effort", "thought_level", "high", ["low", "medium", "high", "max"]),
          select("mode", "mode", "acceptEdits", ["acceptEdits"], names: ["Accept edits"]),
        ]
      client.upsertPane(
        Pane(
          paneId: "pane", workspaceId: "w", name: codex ? "Codex" : "Claude", cwd: "/",
          mode: .agent, state: codex ? .waiting_for_you : .idle, source: .protocol,
          confidence: 1, phrase: "", generation: 1, cols: 80, rows: 24, record: false,
          agent: AgentSession(
            harness: codex ? "registry.codex-acp" : "registry.claude-acp", version: "1",
            fingerprint: "f", options: options, authMethods: [], queue: [], draft: false,
            activePromptId: codex ? "p1" : nil, role: .worker)))
      var view = AgentPaneView()
      view.messages = [
        tool("1", codex ? "Read schema/v2/meta.json" : "Edit acp/client.rs (+84 −3)", .completed),
        tool(
          "2", codex ? "cargo test -p acp contract_session_new" : "Asked Codex to run the tests",
          codex ? .in_progress : .completed),
        TranscriptMessage(
          localId: "3", generation: 1, seq: "3", role: .assistant, kind: .usage, text: "",
          complete: true,
          usage: TokenUsage(totalTokens: codex ? "36000" : "84000", contextSize: "200000")),
      ]
      client.agentPanes["pane"] = view
      return client
    }

    static func host(
      _ scenario: Scenario, _ style: AgentConversationStyle, client: EngineClient? = nil,
      prompt: String = "", alignment: Alignment = .top
    ) -> some View {
      ComposerHost(client: client ?? Self.client(scenario), style: style, prompt: prompt)
        .padding(interiorPadding)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: alignment)
        .background(style.face)
    }

    /// The approved header row: state dot, name, ticker and status word.
    static func header(_ scenario: Scenario, _ style: AgentConversationStyle) -> some View {
      let codex = scenario == .codexRunning
      return HStack(spacing: 8) {
        Circle().fill(codex ? style.accent : style.success).frame(width: 8, height: 8)
        Text(codex ? "Codex" : "Claude").font(PaneInteriorFont.ui(15, weight: .heavy))
          .foregroundStyle(style.ink)
        AgentPaneTickerView(
          client: client(scenario), paneId: "pane",
          font: PaneInteriorFont.ui(12, weight: .medium),
          timeFont: PaneInteriorFont.mono(11, weight: .medium), color: style.secondary)
        Text(codex ? "needs you" : "idle").font(PaneInteriorFont.ui(11, weight: .semibold))
          .foregroundStyle(codex ? style.accent : style.secondary)
      }
      .padding(interiorPadding).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
      .background(style.face)
    }

    private static func select(
      _ id: String, _ category: String, _ current: String, _ values: [String],
      names: [String]? = nil
    ) -> ConfigOption {
      ConfigOption(
        id: id, name: id, category: category, type: .select, currentValue: .v0(current),
        options: zip(values, names ?? values).map { ConfigChoice(value: $0, name: $1) })
    }

    private static func tool(_ id: String, _ title: String, _ status: ToolStatus)
      -> TranscriptMessage
    {
      PaneInteriorFixture.tool(id, title: title, status: status)
    }
  }

  private struct ComposerHost: View {
    let client: EngineClient
    let style: AgentConversationStyle
    @State var prompt: String
    @State private var attachments = AgentComposerAttachments(linkHostFiles: true)
    @FocusState private var focused: Bool

    var body: some View {
      let agent = client.panes.first?.agent
      AgentComposer(
        client: client, paneId: "pane", style: style, prompt: $prompt, attachments: attachments,
        focus: $focused,
        harnessControl: AgentHarnessChipLabel(
          control: HarnessSwitchModel.control(
            current: agent?.harness ?? "", switchState: nil, harnesses: [],
            paneName: client.panes.first?.name),
          harnessId: agent?.harness ?? "", style: style),
        connected: true, submit: {}
      )
      // As inside `AgentConversationView`: the chips' scroll view gets overlay scrollers.
      .overlayScrollersWithin()
    }
  }
#endif
