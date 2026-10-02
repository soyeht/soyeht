import SwiftUI
import Testing

@testable import SoyehtClient

#if os(macOS)
  import AppKit
  import CoreText
#endif

/// The approved pane interior resolves every color from the active palette's roles, in all
/// nine approved palettes, and its text roles stay readable on the surfaces they sit on.
@MainActor @Test(arguments: TerminalColorTheme.allCases)
func paneInteriorColorsResolveFromThePaletteRoles(theme: TerminalColorTheme) {
  let palette = theme.palette
  #expect(palette.isSelfConsistent, "\(theme)")
  // agent-0…3 and the ansi danger, success and warning roles exist in every palette.
  #expect(palette.agentPlates.count >= 4, "\(theme)")
  #expect(palette.ansi.count == 16, "\(theme)")

  let colors = SoyehtThemeColorSet(palette: palette)
  let style = AgentConversationStyle.pane(colors)
  let roles: [(String, Color, Color)] = [
    ("canvas", style.canvas, colors.canvas), ("face", style.face, colors.face),
    ("well", style.well, colors.well), ("border", style.border, colors.border),
    ("ink", style.ink, colors.ink), ("ink-secondary", style.secondary, colors.secondary),
    ("ink-muted", style.muted, colors.muted), ("accent", style.accent, colors.accent),
    ("success", style.success, colors.success), ("warning", style.warning, colors.warning),
    ("danger", style.danger, colors.danger),
  ]
  for (role, used, token) in roles {
    #expect(used == token, "\(theme) \(role)")
  }
  #expect(colors.border == colors.well, "\(theme): the approved border role is the well tone")
}

@Test(arguments: TerminalColorTheme.allCases)
func paneInteriorTextIsReadableInEveryPalette(theme: TerminalColorTheme) {
  let palette = theme.palette
  let contrast = ThemeLabColorMath.contrastRatio
  // Body text on cards, the primary pill's label on its ink fill, secondary pills' labels.
  #expect(contrast(palette.ink, palette.canvas) >= 4.5, "\(theme) ink on canvas")
  #expect(contrast(palette.ink, palette.face) >= 4.5, "\(theme) ink on face")
  #expect(contrast(palette.face, palette.ink) >= 4.5, "\(theme) face on ink")
  #expect(contrast(palette.ink, palette.well) >= 4.5, "\(theme) ink on well")
}

/// Deny is `danger` text on a `well` pill. The approved palettes do not reach 4.5:1 there;
/// this records the gap so a palette change that closes or widens it is noticed.
@Test func dangerTextOnWellRecordsTheApprovedPalettesContrastGap() {
  let below = TerminalColorTheme.allCases.filter {
    ThemeLabColorMath.contrastRatio($0.palette.danger, $0.palette.well) < 4.5
  }
  #expect(
    below == [
      .neoMilk, .neoPaleMist, .neoMistyBlue, .neoSunlitChartreuse, .neoSunriseGold,
      .neoDeepVine, .neoDeepForest, .neoDeepHarbor,
    ])
  for theme in TerminalColorTheme.allCases {
    #expect(ThemeLabColorMath.contrastRatio(theme.palette.danger, theme.palette.well) >= 2.4)
  }
}

#if os(macOS)
  /// Renders the approved agent interior in every palette. The background is the palette's
  /// face, the cards its canvas, and no pixel is white: the flat interior draws no highlight.
  /// With `SOYEHT_DESIGN_SHOTS_DIR` set, it also writes each render there as a PNG.
  @MainActor @Test(arguments: TerminalColorTheme.allCases)
  func agentPaneInteriorRendersFlatInEveryPalette(theme: TerminalColorTheme) throws {
    PaneInteriorRendering.registerFonts()
    let colors = SoyehtThemeColorSet(palette: theme.palette)
    let view = AgentConversationView(
      client: PaneInteriorFixture.client(), paneId: PaneInteriorFixture.paneId,
      showsConfiguration: false, style: .pane(colors))
    let bitmap = try #require(
      PaneInteriorRendering.bitmap(view, size: CGSize(width: 632, height: 560)))
    let pixels = PaneInteriorRendering.histogram(bitmap)
    let palette = theme.palette
    #expect(PaneInteriorRendering.share(of: palette.face, in: pixels) > 0.2, "\(theme) face")
    #expect(PaneInteriorRendering.share(of: palette.canvas, in: pixels) > 0.05, "\(theme) canvas")
    #expect(PaneInteriorRendering.share(of: "#FFFFFF", in: pixels) == 0, "\(theme) white")
    if let directory = ProcessInfo.processInfo.environment["SOYEHT_DESIGN_SHOTS_DIR"] {
      let url = URL(fileURLWithPath: directory)
        .appendingPathComponent("agent-pane-\(theme.rawValue).png")
      try bitmap.representation(using: .png, properties: [:])?.write(to: url)
    }
  }

  @MainActor enum PaneInteriorRendering {
    private static var fontsRegistered = false

    /// The Mac app bundles these faces; the test registers them from the repository.
    static func registerFonts() {
      guard !fontsRegistered else { return }
      fontsRegistered = true
      let fonts = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().appendingPathComponent("mac/Resources/Fonts")
      let files = (try? FileManager.default.contentsOfDirectory(atPath: fonts.path)) ?? []
      for file in files where file.hasSuffix(".ttf") {
        let url = fonts.appendingPathComponent(file) as CFURL
        CTFontManagerRegisterFontsForURL(url, .process, nil)
      }
    }

    /// Renders through AppKit at 2x, so scroll views and text fields draw as on screen.
    static func bitmap(_ view: some View, size: CGSize) -> NSBitmapImageRep? {
      let host = NSHostingView(rootView: view.frame(width: size.width, height: size.height))
      host.frame = CGRect(origin: .zero, size: size)
      let window = NSWindow(
        contentRect: host.frame, styleMask: .borderless, backing: .buffered, defer: false)
      window.contentView = host
      for _ in 0..<6 {
        host.layoutSubtreeIfNeeded()
        RunLoop.current.run(until: Date().addingTimeInterval(0.05))
      }
      guard
        let bitmap = NSBitmapImageRep(
          bitmapDataPlanes: nil, pixelsWide: Int(size.width * 2), pixelsHigh: Int(size.height * 2),
          bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
          colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
      else { return nil }
      bitmap.size = size
      host.cacheDisplay(in: host.bounds, to: bitmap)
      return bitmap.retagging(with: .sRGB)
    }

    static func histogram(_ bitmap: NSBitmapImageRep) -> [UInt32: Int] {
      var counts: [UInt32: Int] = [:]
      guard let data = bitmap.bitmapData else { return counts }
      let pixelBytes = bitmap.bitsPerPixel / 8
      // Every second pixel of the 2x render: one sample per point keeps the test fast.
      for y in stride(from: 0, to: bitmap.pixelsHigh, by: 2) {
        let row = data + y * bitmap.bytesPerRow
        for x in stride(from: 0, to: bitmap.pixelsWide, by: 2) {
          let pixel = row + x * pixelBytes
          let key = UInt32(pixel[0]) << 16 | UInt32(pixel[1]) << 8 | UInt32(pixel[2])
          counts[key, default: 0] += 1
        }
      }
      return counts
    }

    /// Share of pixels within 2/255 per channel of a palette hex value.
    static func share(of hex: String, in histogram: [UInt32: Int]) -> Double {
      guard let rgb = ThemeHexColorMath.rgb8(hex) else { return 0 }
      let total = histogram.values.reduce(0, +)
      let near = histogram.filter { key, _ in
        abs(Int(key >> 16 & 0xFF) - Int(rgb.0)) <= 2
          && abs(Int(key >> 8 & 0xFF) - Int(rgb.1)) <= 2
          && abs(Int(key & 0xFF) - Int(rgb.2)) <= 2
      }.values.reduce(0, +)
      return total == 0 ? 0 : Double(near) / Double(total)
    }
  }
#endif
