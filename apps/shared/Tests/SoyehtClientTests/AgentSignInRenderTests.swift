#if os(macOS)
  import AppKit
  import SoyehtProtocol
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The sign-in card and the account menu render flat in the palette ("Apple limpo": canvas
  /// cards, ink primary buttons, no white pixel) in the light and dark themes. With
  /// `SOYEHT_DESIGN_SHOTS_DIR` set, each render is written there as a PNG.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func signInCardAndAccountMenuRenderFlatInThePalette(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let palette = theme.palette
    let style = AgentConversationStyle.pane(SoyehtThemeColorSet(palette: palette))
    let client = EngineClient(socket: "/tmp/sn-sign-in-shot/engine.sock")
    let methods = [
      AuthMethod(id: "subscription", name: "Subscription", terminal: true),
      AuthMethod(id: "console", name: "Console", terminal: true),
    ]
    func card(_ state: AgentSignInState, height: CGFloat) -> (AnyView, CGSize) {
      let view = AgentSignInCard(
        client: client, paneId: "pane", state: state, style: style,
        inset: PaneInteriorMetrics.cardPadding
      )
      .padding(.top, PaneInteriorMetrics.cardPadding)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
      .background(style.face)
      return (AnyView(view), CGSize(width: 678, height: height))
    }
    let account = AgentAccountActions(
      agent: AgentSession(
        harness: "agent", version: "1", fingerprint: "f", sessionId: "s", options: [],
        authMethods: [], queue: [], draft: false, role: .worker, reauthMethods: methods,
        canSignOut: true),
      paneState: .idle, connected: true)
    let menu = AgentAccountPanel(
      actions: try #require(account), style: style, run: { _ in }
    )
    .frame(width: 448).frame(maxHeight: .infinity, alignment: .top).background(style.face)
    let shots: [(String, AnyView, CGSize)] = [
      (
        "sign-in-choose",
        card(
          .choose(
            methods: methods.map(AgentSignInMethod.init),
            failure:
              "Sign-in stopped because Soyeht's background services restarted. Sign in again."
          ), height: 260
        ).0, CGSize(width: 678, height: 260)
      ),
      (
        "sign-in-waiting", card(.waiting(terminalPaneId: "terminal"), height: 150).0,
        CGSize(width: 678, height: 150)
      ),
      ("sign-in-verifying", card(.verifying, height: 120).0, CGSize(width: 678, height: 120)),
      (
        "sign-in-failed",
        card(.failed(reason: "Sign-in was cancelled."), height: 140).0,
        CGSize(width: 678, height: 140)
      ),
      ("account-menu", AnyView(menu), CGSize(width: 448, height: 240)),
    ]
    for (name, view, size) in shots {
      let pixels = try PaneInteriorRendering.shot(name, view, size: size, theme: theme)
      #expect(PaneInteriorRendering.share(of: palette.face, in: pixels) > 0.1, "\(theme) \(name)")
    }
  }
#endif
