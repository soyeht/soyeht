import SoyehtProtocol

/// Soft wrap for the paged code view, shared by the Mac and the iPad.
///
/// The view holds one page of logical lines (`EditorViewportPaging`). With soft wrap a page line
/// takes one or more rows of `columns` cells. Lines outside the page are not loaded, so they
/// count one row each; the estimate only moves the scroll extent, and the page that comes into
/// view is measured when it loads. Wrapping is by character in a monospaced font: every Unicode
/// scalar takes one cell, combining marks none, a tab runs to the next multiple of `tabWidth`
/// within its row, and a space past the row end hangs there instead of starting a row, which is
/// how the Mac text view breaks lines with
/// `EditorTextLayout.paragraphStyle`. Line numbers stay on logical lines: the gutter labels the
/// first row of each line.
///
/// Building a layout walks the page text once. Typing rebuilds it for the page only, never for
/// the whole file.
public struct EditorWrapLayout: Equatable, Sendable {
  public static let tabWidth = ClientPolicy.editorTabWidth

  /// Cells per row, or nil when lines do not wrap.
  public let columns: Int?
  /// The UTF-16 offset where each page line starts, then the page length plus one.
  public private(set) var lineStarts: [Int]
  /// The row where each page line starts, then the page's row count.
  public private(set) var rowStarts: [Int]

  public init(text: String, columns: Int?) {
    self.columns = columns.map { max(1, $0) }
    lineStarts = [0]
    rowStarts = [0]
    var offset = 0
    var column = 0
    var rows = 1
    for scalar in text.unicodeScalars {
      offset += scalar.utf16.count
      if scalar == "\n" {
        lineStarts.append(offset)
        rowStarts.append(rowStarts[rowStarts.count - 1] + rows)
        column = 0
        rows = 1
        continue
      }
      guard let columns = self.columns else { continue }
      var width = Self.cells(scalar, at: column)
      // A space hangs past the row end, as in AppKit; the next character starts the new row.
      if scalar != " ", column > 0, column + width > columns {
        rows += 1
        column = 0
        width = Self.cells(scalar, at: 0)
      }
      column += width
    }
    lineStarts.append(offset + 1)
    rowStarts.append(rowStarts[rowStarts.count - 1] + rows)
  }

  public var lineCount: Int { lineStarts.count - 1 }
  public var rowCount: Int { rowStarts[rowStarts.count - 1] }

  public func rows(ofLine index: Int) -> Int {
    rowStarts[index + 1] - rowStarts[index]
  }

  /// The page line shown at `row` of the page, clamped to the page.
  public func line(atRow row: Int) -> Int {
    Self.lastIndex(in: rowStarts, notAfter: row, count: lineCount)
  }

  /// The page line holding UTF-16 `offset` of the page text, clamped to the page.
  public func line(containing offset: Int) -> Int {
    Self.lastIndex(in: lineStarts, notAfter: offset, count: lineCount)
  }

  /// Rows of the whole document when this page starts at `fromLine`: unloaded lines count one.
  public func documentRows(totalLines: UInt32) -> Int {
    max(Int(totalLines), lineCount) - lineCount + rowCount
  }

  /// The logical line shown at `row` of the whole document.
  public func logicalLine(atDocumentRow row: Int, fromLine: UInt32) -> UInt32 {
    let first = Int(fromLine)
    if row < first { return UInt32(clamping: max(0, row)) }
    let inPage = row - first
    if inPage < rowCount { return UInt32(clamping: first + line(atRow: inPage)) }
    return UInt32(clamping: first + lineCount + inPage - rowCount)
  }

  /// Cells per row that fit `width` points of characters `characterWidth` points wide.
  public static func columns(fitting width: Double, characterWidth: Double) -> Int {
    guard characterWidth > 0, width.isFinite else { return 1 }
    return max(1, Int((width / characterWidth).rounded(.down)))
  }

  private static func cells(_ scalar: Unicode.Scalar, at column: Int) -> Int {
    if scalar == "\t" { return tabWidth - column % tabWidth }
    if scalar.isASCII { return 1 }
    switch scalar.properties.generalCategory {
    case .nonspacingMark, .enclosingMark: return 0
    default: return 1
    }
  }

  /// The last index `i < count` with `starts[i] <= value`, or 0.
  private static func lastIndex(in starts: [Int], notAfter value: Int, count: Int) -> Int {
    var low = 0
    var high = count - 1
    while low < high {
      let middle = (low + high + 1) / 2
      if starts[middle] <= value { low = middle } else { high = middle - 1 }
    }
    return max(0, low)
  }
}
