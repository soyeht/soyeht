import Testing

@testable import SoyehtClient

@Test func nineProductionThemesAreClosedNamedAndSelfConsistent() {
  #expect(
    TerminalColorTheme.allCases.map(\.displayName) == [
      "Neo · Milk", "Neo · Pale Mist", "Neo · Misty Blue", "Neo · Sunlit Chartreuse",
      "Neo · Sunrise Gold", "Neo · Deep Vine", "Neo · Deep Forest", "Neo · Midnight Teal",
      "Neo · Deep Harbor",
    ])
  #expect(TerminalColorTheme.seededDefault == .neoMilk)
  #expect(TerminalColorTheme.allCases.allSatisfy { $0.palette.isSelfConsistent })
}

@Test func neoMilkMatchesThePreSliceRewriteTokensByteForByte() {
  let palette = TerminalColorTheme.neoMilk.palette
  #expect(palette.canvas == "#E0E5EC")
  #expect(palette.face == "#E8EDF4")
  #expect(palette.well == "#D8DEE8")
  #expect(palette.hover == "#DCE2EA")
  #expect(palette.ink == "#3E4A66")
  #expect(palette.secondary == "#6E7A96")
  #expect(palette.muted == "#8E9AB3")
  #expect(palette.accent == "#5B7CFA")
  #expect(palette.onAccent == "#FFFFFF")
  #expect(palette.shadowDark == "#A6B4C8")
  #expect(palette.shadowLight == "#FFFFFF")
  #expect(palette.selection == "#C9D6EE")
  #expect(palette.success == "#2E9A73")
  #expect(palette.warning == "#B98A2E")
  #expect(palette.danger == "#D9556A")
  #expect(palette.syntaxType == "#8E5BFA")
  #expect(palette.diffAdded == "#3F9C6A")
  #expect(palette.agentPlates == ["#D8E0FE", "#CDE7DD", "#F6D6DB", "#EEE3CD"])
}

@Test func raisedAndWellPairsMatchTheProductionThemes() {
  let expected: [TerminalColorTheme: [String?]] = [
    .neoMilk: ["#A6B4C8", "#FFFFFF", "#A6B4C8", "#FFFFFF", nil],
    .neoPaleMist: ["#8C9395", "#D1D9DB", "#8C9395", "#D1D9DB", nil],
    .neoMistyBlue: ["#517480", "#93B7C5", "#517480", "#93B7C5", nil],
    .neoSunlitChartreuse: ["#A2A13C", "#ECE77F", "#A2A13C", "#ECE77F", nil],
    .neoSunriseGold: ["#B38531", "#FFCA73", "#B38531", "#FFCA73", nil],
    .neoDeepVine: ["#22240F", "#4A4C35", "#131400", "#3F412A", "#060700"],
    .neoDeepForest: ["#0B240B", "#324C32", "#001200", "#264126", "#000100"],
    .neoMidnightTeal: ["#000F18", "#1A3746", "#000104", "#0B2A39", "#000104"],
    .neoDeepHarbor: ["#112F37", "#3B5861", "#001E26", "#304D56", "#00151B"],
  ]

  for theme in TerminalColorTheme.allCases {
    let palette = theme.palette
    #expect(
      [
        palette.shadowDark, palette.shadowLight, palette.wellShadow, palette.wellRim,
        palette.wellLip,
      ]
        == expected[theme])
  }
}

@Test func perceptualDarknessMatchesTheProductionClassification() {
  let dark: Set<TerminalColorTheme> = [
    .neoDeepVine, .neoDeepForest, .neoMidnightTeal, .neoDeepHarbor,
  ]
  for theme in TerminalColorTheme.allCases {
    #expect(theme.palette.isDark == dark.contains(theme))
  }
}

@Test func sharedHexAndShadowMathMatchTheProductionRules() throws {
  #expect(ThemeHexColorMath.mix("#000000", "#FFFFFF", amount: 0.5) == "#808080")
  #expect(ThemeHexColorMath.lighten("#000000", by: 0.25) == "#404040")
  #expect(ThemeHexColorMath.darken("#FFFFFF", by: 0.25) == "#BFBFBF")
  #expect(ThemeHexColorMath.withAlpha("#5B7CFA", 0.4) == "#5B7CFA66")
  #expect(ThemeShadowMath.renderRadius(blur: 18) == 9)
  #expect(ThemeLabColorMath.contrastRatio("#000000", "#FFFFFF") == 21)
  let accent = try #require(ThemeLabColorMath.lch(of: "#5B7CFA"))
  #expect(ThemeLabColorMath.hex(accent) == "#5B7CFA")
  #expect(ThemeLabColorMath.withLightness("#5B7CFA", 50) != nil)
  #expect((ThemeLabColorMath.distance("#000000", "#FFFFFF") ?? 0) > 99)
}
