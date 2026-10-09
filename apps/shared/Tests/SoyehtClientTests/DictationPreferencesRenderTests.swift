#if os(macOS)
  import AppKit
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  @MainActor private final class SilentRecognizer: SpeechRecognizing {
    func authorize() async -> DictationAuthorization { .authorized }
    func start(
      _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
    ) throws {}
    func finish() {}
    func cancel() {}
  }

  /// Preferences > Dictation draws flat in the palette, with no white pixel, whatever the
  /// catalogue holds: one engine that needs an install, one that cannot run here, and Apple's.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoDeepHarbor])
  func dictationPreferencesRenderFlatInThePalette(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let download = DictationModelInfo(
      sizeDescription: "about 642 MB", licenseName: "Test licence",
      licenseURL: URL(string: "https://example.com/licence"), install: { _ in },
      isInstalled: { false }, remove: {})
    let big = DictationEngine(
      id: "big", name: "Big engine", badge: "Big", summary: "Needs a download.",
      platforms: [.mac], languages: [.system, .automatic], usesVocabulary: true, model: download,
      make: { SilentRecognizer() })
    let phone = DictationEngine(
      id: "phone", name: "Phone engine", badge: "Phone", summary: "Phones only.",
      platforms: [.iPhone], languages: [.system], make: { SilentRecognizer() })
    let defaults = UserDefaults(suiteName: "dictation-render-\(UUID().uuidString)") ?? .standard
    let store = DictationEngineStore(
      registry: DictationEngineRegistry(fallback: .apple, others: [big, phone]),
      defaults: defaults, platform: .mac)
    // The shortcut found only Apple installed: Preferences opens at the install step.
    store.cycle()
    store.vocabulary.add("Soyeht")

    let style = AgentConversationStyle.pane(SoyehtThemeColorSet(palette: theme.palette))
    let view = DictationPreferencesView(store: store, style: style)
      .padding(PaneInteriorMetrics.cardPadding)
      .frame(width: 600, height: 620, alignment: .topLeading)
      .background(style.face)
    let bitmap = try #require(
      PaneInteriorRendering.bitmap(view, size: CGSize(width: 600, height: 620)))
    let pixels = PaneInteriorRendering.histogram(bitmap)
    #expect(PaneInteriorRendering.share(of: "#FFFFFF", in: pixels) == 0, "\(theme)")
    #expect(PaneInteriorRendering.share(of: theme.palette.canvas, in: pixels) > 0.1, "\(theme)")
    #expect(PaneInteriorRendering.share(of: theme.palette.ink, in: pixels) > 0, "\(theme)")
    if let directory = ProcessInfo.processInfo.environment["SOYEHT_DESIGN_SHOTS_DIR"] {
      let url = URL(fileURLWithPath: directory)
        .appendingPathComponent("dictation-preferences-\(theme.rawValue).png")
      try bitmap.representation(using: .png, properties: [:])?.write(to: url)
    }
  }
#endif
