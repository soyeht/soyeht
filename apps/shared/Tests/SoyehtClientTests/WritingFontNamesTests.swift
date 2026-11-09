import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func writingFontsResolveFromTheTokenFamiliesInTheOrderTheClientsTry() {
  let reading = DesignTokens.Fonts.reading
  let chrome = DesignTokens.Fonts.chrome
  #expect(WritingFontNames.candidates(for: .reading_serif) == [reading, "\(reading)-Regular"])
  #expect(
    WritingFontNames.candidates(for: .classic_serif) == [
      DesignTokens.Fonts.classicSerifPostScript
    ])
  #expect(WritingFontNames.candidates(for: .sans) == ["\(chrome)-Regular", chrome])
}

@Test func screenplayFontsNameTheBundledCourierFaces() {
  let family = DesignTokens.Fonts.screenplay
  let spaceless = family.replacingOccurrences(of: " ", with: "")
  #expect(
    WritingFontNames.screenplayCandidates(bold: false) == [family, "\(spaceless)-Regular"])
  #expect(
    WritingFontNames.screenplayCandidates(bold: true) == ["\(spaceless)-Bold", "\(family) Bold"])
}
