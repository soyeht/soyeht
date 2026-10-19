#if os(macOS)
  import AppKit
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  private func width(
    dictation: Bool, context: Bool, percentage: Bool
  ) -> Double {
    let spacing = Double(AgentComposerMetrics.controlSpacing)
    let button = Double(AgentComposerMetrics.controlHeight)
    var total =
      button + Double(AgentComposerMetrics.sendWidth) + AgentComposerMetrics.chipsMinimumWidth
      + 2 * spacing
    if dictation { total += button + spacing }
    if context {
      total +=
        Double(
          AgentComposerMetrics.ringDiameter + 2 * AgentComposerMetrics.contextHorizontalPadding)
        + spacing
    }
    if percentage {
      total +=
        Double(AgentComposerMetrics.contextSpacing) + AgentComposerMetrics.contextPercentageWidth
    }
    return total
  }

  /// The control row gives things up in a fixed order as it narrows: the gauge's percentage,
  /// then the gauge, then the mic. "+", the chips at their minimum and send always stay.
  @Test func theControlRowGivesUpThePercentageThenTheGaugeThenTheMic() {
    let full = width(dictation: true, context: true, percentage: true)
    let ringOnly = width(dictation: true, context: true, percentage: false)
    let noGauge = width(dictation: true, context: false, percentage: false)
    let bare = width(dictation: false, context: false, percentage: false)
    func plan(_ width: Double) -> AgentComposerRowPlan {
      .plan(rowWidth: width, hasContext: true)
    }

    #expect(
      plan(full)
        == AgentComposerRowPlan(
          showsContext: true, showsContextPercentage: true, showsDictation: true))
    #expect(
      plan(full - 1)
        == AgentComposerRowPlan(
          showsContext: true, showsContextPercentage: false, showsDictation: true))
    #expect(plan(ringOnly - 1).showsContext == false)
    #expect(plan(ringOnly - 1).showsDictation)
    #expect(plan(noGauge - 1).showsDictation == false)
    #expect(plan(bare) == plan(noGauge - 1))
    #expect(plan(0).showsContext == false && plan(0).showsDictation == false)
  }

  @Test func aRowWithoutContextNeverShowsTheGauge() {
    let plan = AgentComposerRowPlan.plan(rowWidth: 1000, hasContext: false)
    #expect(!plan.showsContext && !plan.showsContextPercentage && plan.showsDictation)
  }

  /// The composer of a narrow pane: the control row keeps the chips readable and the placeholder
  /// stays on one line. With `SOYEHT_DESIGN_SHOTS_DIR` set, each render is written there.
  @MainActor @Test(arguments: [300.0, 340.0, 480.0])
  func theComposerFitsANarrowPane(paneWidth: Double) throws {
    PaneInteriorRendering.registerFonts()
    let colors = SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette)
    let client = PaneInteriorFixture.client()
    let view = AgentConversationView(
      client: client, paneId: PaneInteriorFixture.paneId, showsConfiguration: false,
      style: .pane(colors)
    ) { Text("Codex") }
    .padding(PaneLayoutContentMetrics.paneCardInset).paneShell(colors)
    .padding(PaneLayoutSpacing.outerMargin).background(colors.canvas)
    let size = CGSize(
      width: paneWidth + 2 * PaneLayoutSpacing.outerMargin,
      height: 560)
    let bitmap = try #require(PaneInteriorRendering.bitmap(view, size: size))
    if let directory = ProcessInfo.processInfo.environment["SOYEHT_DESIGN_SHOTS_DIR"] {
      let url = URL(fileURLWithPath: directory)
        .appendingPathComponent("composer-\(Int(paneWidth)).png")
      try bitmap.representation(using: .png, properties: [:])?.write(to: url)
    }
  }
#endif
