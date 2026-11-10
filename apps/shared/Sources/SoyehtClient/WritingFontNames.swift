import Foundation
import SoyehtProtocol

/// The names a platform font lookup tries, in order, for the faces a writing document uses. The
/// families are tokens of `contracts/design-tokens.json`; only the way a bundled face is spelled
/// (family name, or PostScript name) lives here, so no client types a font name.
public enum WritingFontNames {
  /// The faces of a document's font; a client uses the first one it can resolve.
  public static func candidates(for font: WritingFont) -> [String] {
    switch font {
    case .reading_serif:
      [DesignTokens.Fonts.reading, postScript(DesignTokens.Fonts.reading, style: "Regular")]
    case .classic_serif:
      [DesignTokens.Fonts.classicSerifPostScript]
    case .sans:
      [postScript(DesignTokens.Fonts.chrome, style: "Regular"), DesignTokens.Fonts.chrome]
    }
  }

  /// The faces of a screenplay's Courier page, regular or bold.
  public static func screenplayCandidates(bold: Bool) -> [String] {
    let family = DesignTokens.Fonts.screenplay
    if bold { return [postScript(family, style: "Bold"), "\(family) Bold"] }
    return [family, postScript(family, style: "Regular")]
  }

  /// `Family Name` and `Style` as the PostScript name of a bundled face: `FamilyName-Style`.
  private static func postScript(_ family: String, style: String) -> String {
    family.filter { !$0.isWhitespace } + "-" + style
  }
}
