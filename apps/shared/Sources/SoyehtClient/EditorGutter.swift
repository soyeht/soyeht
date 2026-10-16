import Foundation

/// How wide the line-number gutter is. The cases are `contracts/vectors/editor.json`, which the
/// desktop client passes as well (`soyeht_client::editor::gutter`).
///
/// The width follows the digits of the file's last line number, so that no number is clipped: a
/// number is `digits * digitWidth` wide, with the text inset on both sides, and the gutter is
/// never narrower than its minimum (the `gutterWidth` token).
public enum EditorGutter {
  /// The decimal digits of the last line number of a file of `lineCount` lines; an empty file
  /// has one.
  static func digits(_ lineCount: UInt32) -> Int {
    String(max(lineCount, 1)).count
  }

  /// The width of the gutter in points, where a digit is `digitWidth` wide as the font was
  /// measured.
  public static func width(
    lineCount: UInt32, digitWidth: Double,
    minimum: Double = PaneLayoutContentMetrics.editorGutterWidth,
    inset: Double = PaneLayoutContentMetrics.editorTextInset
  ) -> Double {
    max(minimum, (Double(digits(lineCount)) * digitWidth).rounded(.up) + 2 * inset)
  }
}
