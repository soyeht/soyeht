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
#endif
