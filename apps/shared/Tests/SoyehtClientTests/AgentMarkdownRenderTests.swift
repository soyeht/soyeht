#if os(macOS)
  import AppKit
  import SoyehtProtocol
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// An agent's reply with Markdown renders flat in the palette: code sits in the `well` and no pixel
  /// is white. With `SOYEHT_DESIGN_SHOTS_DIR` set, each render is
  /// written there as a PNG.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func anAgentReplyWithMarkdownRendersFlatInThePalette(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let client = PaneInteriorFixture.client(messages: [
      PaneInteriorFixture.message(
        "1", role: .user, text: "Summarize the change and show the new render function."),
      PaneInteriorFixture.message("2", role: .assistant, text: PaneInteriorFixture.markdownReply),
    ])
    client.agentPanes[PaneInteriorFixture.paneId]?.permission = nil
    // A reply still arriving: its open code span is drawn closed.
    let streaming = TranscriptMessage(
      localId: "3", generation: 1, seq: "3", role: .assistant, kind: .message,
      text: "Now running `cargo te", complete: false)
    client.agentPanes[PaneInteriorFixture.paneId]?.messages.append(streaming)
    let view = AgentConversationView(
      client: client, paneId: PaneInteriorFixture.paneId, showsConfiguration: false,
      style: .pane(SoyehtThemeColorSet(palette: theme.palette)))
    let bitmap = try #require(
      PaneInteriorRendering.bitmap(view, size: CGSize(width: 632, height: 760)))
    let pixels = PaneInteriorRendering.histogram(bitmap)
    let palette = theme.palette
    #expect(PaneInteriorRendering.share(of: palette.face, in: pixels) > 0.2, "\(theme) face")
    #expect(PaneInteriorRendering.share(of: palette.well, in: pixels) > 0.01, "\(theme) well")
    #expect(PaneInteriorRendering.share(of: "#FFFFFF", in: pixels) == 0, "\(theme) white")
    if let directory = ProcessInfo.processInfo.environment["SOYEHT_DESIGN_SHOTS_DIR"] {
      let url = URL(fileURLWithPath: directory)
        .appendingPathComponent("agent-markdown-\(theme.rawValue).png")
      try bitmap.representation(using: .png, properties: [:])?.write(to: url)
    }
  }

  /// The editor's Markdown preview draws through the same view with inert links: the blocks stay
  /// apart, the code block sits in the `well`, and no pixel is white.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func aMarkdownPreviewDrawsItsBlocksInThePalette(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let blocks = MarkdownReply.parse(
      "# Title\nSome **bold** text and a [link](https://example.com).\n- one\n- two\n\n"
        + "```\nlet x = 1\n```")
    let style = AgentConversationStyle.pane(SoyehtThemeColorSet(palette: theme.palette))
    let view = MarkdownBlocksView(blocks: blocks, style: style)
      .padding(PaneInteriorMetrics.cardPadding)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
      .background(style.face)
    let pixels = try PaneInteriorRendering.shot(
      "markdown-preview", view, size: CGSize(width: 360, height: 300), theme: theme)
    #expect(PaneInteriorRendering.share(of: theme.palette.well, in: pixels) > 0.01, "\(theme) well")
  }

  /// A link in an inert document (the editor preview) looks like a link, and opens nothing.
  @MainActor @Test func aLinkThatDoesNotOpenKeepsItsLookWithoutALink() throws {
    let style = AgentConversationStyle.pane(
      SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
    let runs = [MarkdownRun("docs", link: "https://example.com")]
    let inert = MarkdownAttributedString.make(runs, style: style, opensLinks: false)
    #expect(inert.runs.allSatisfy { $0.link == nil })
    #expect(inert.runs.allSatisfy { $0.foregroundColor == style.accent })
    #expect(inert.runs.allSatisfy { $0.underlineStyle == .single })
    let live = MarkdownAttributedString.make(runs, style: style)
    #expect(live.runs.allSatisfy { $0.link == URL(string: "https://example.com") })
  }
#endif
