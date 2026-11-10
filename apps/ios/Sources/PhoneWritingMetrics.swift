import SoyehtClient
import SoyehtProtocol
import SwiftUI
import UIKit

@MainActor
enum PhoneWritingMetrics {
  static let screenplayMetric: PaneLayoutFontMetric = {
    let font = screenplayFont()
    let width = ("0" as NSString).size(withAttributes: [.font: font]).width
    return PaneLayoutFontMetric(
      characterWidth: ceil(width * 1_000) / 1_000,
      lineHeight: PaneLayoutContentMetrics.screenplayLineHeight,
      pointSize: PaneLayoutContentMetrics.screenplayFontPointSize)
  }()

  static let fonts = PaneLayoutFontMetrics(
    terminal: PaneLayoutFontMetric(characterWidth: 8, lineHeight: 18, pointSize: 13),
    editor: PaneLayoutFontMetric(characterWidth: 6.9, lineHeight: 19, pointSize: 11.5),
    chrome: PaneLayoutFontMetric(characterWidth: 7.2, lineHeight: 17, pointSize: 12),
    agent: PaneLayoutFontMetric(characterWidth: 9, lineHeight: 21, pointSize: 15),
    prose: metric(for: .reading_serif),
    screenplay: screenplayMetric)
  static let floors = PaneLayoutFloorFormula.surfaceFloors(fonts: fonts)
  static let sizing = PaneLayoutSizingPolicy(
    floors: floors, gaps: PaneLayoutSpacing.gaps)
  static let policy = PhoneWritingLayoutPolicy(floors: floors)
  static let chromePolicy = PhoneWritingChromePolicy()
  static let sheetPolicy = PhoneWritingSheetPolicy()
  static let bodyWritingMetrics = PhoneWritingLayoutMetrics.standard.bodyWritingMetrics()
  static let bodyFloors = PhoneWritingLayoutMetrics.standard.bodySurfaceFloors(from: floors)

  static func metric(for document: WritingDocument) -> PaneLayoutFontMetric {
    document.kind == .screenplay ? fonts.screenplay : metric(for: document.font)
  }

  static func metric(for font: WritingFont) -> PaneLayoutFontMetric {
    let uiFont = uiFont(for: font)
    let width = ("0" as NSString).size(withAttributes: [.font: uiFont]).width
    return PaneLayoutFontMetric(
      characterWidth: ceil(width * 1_000) / 1_000,
      lineHeight: PaneLayoutContentMetrics.textLineHeight,
      pointSize: PaneLayoutContentMetrics.textFontPointSize)
  }

  static func uiFont(for font: WritingFont) -> UIFont {
    let size = PaneLayoutContentMetrics.textFontPointSize
    return resolved(WritingFontNames.candidates(for: font), size: size)
      ?? UIFont.systemFont(ofSize: size)
  }

  static func screenplayFont(bold: Bool = false) -> UIFont {
    let size = PaneLayoutContentMetrics.screenplayFontPointSize
    return resolved(WritingFontNames.screenplayCandidates(bold: bold), size: size)
      ?? UIFont.monospacedSystemFont(ofSize: size, weight: bold ? .bold : .regular)
  }

  /// The first face of `names` the system can load.
  private static func resolved(_ names: [String], size: CGFloat) -> UIFont? {
    names.lazy.compactMap { UIFont(name: $0, size: size) }.first
  }
}
