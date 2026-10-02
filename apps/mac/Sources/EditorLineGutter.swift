import AppKit
import SoyehtClient
import SoyehtProtocol

/// Line numbers, the current line, git markers, agent review and agent cursors beside the
/// loaded page. With soft wrap a number labels the first row of its logical line: the shared
/// `EditorWrapLayout` picks the lines near the visible rows, and the text view's own layout
/// places each one, so the numbers sit on the drawn text.
@MainActor final class EditorLineGutter: NSView {
  static let width: CGFloat = PaneLayoutContentMetrics.editorGutterWidth
  private static let rowHeight = EditorTextMetrics.lineHeight
  /// Rows read before the first visible one, in case a wide glyph wrapped earlier than the
  /// shared layout counts.
  private static let wrapSlackRows = 8
  override var isFlipped: Bool { true }
  private weak var textView: NSTextView?
  private weak var scrollView: NSScrollView?
  private var startLine: UInt32 = 0
  private var layout = EditorWrapLayout(text: "", columns: nil)
  private var diffMarkers: [UInt32: Set<EditorDiffMarker>] = [:]
  private var agentReviewLines: Set<UInt32> = []
  private var agentCursors: [UInt32: String] = [:]
  var selectedLine: UInt32?

  init(scrollView: NSScrollView, textView: NSTextView) {
    self.scrollView = scrollView
    self.textView = textView
    super.init(frame: .zero)
    wantsLayer = true
  }

  required init?(coder: NSCoder) {
    super.init(coder: coder)
  }

  func update(
    startLine: UInt32, markers: [UInt32: Set<EditorDiffMarker>],
    agentReviewLines: Set<UInt32>, agentCursors: [EditorAgentCursorEvent], selectedLine: UInt32?
  ) {
    self.startLine = startLine
    diffMarkers = markers
    self.agentReviewLines = agentReviewLines
    self.agentCursors = Dictionary(
      agentCursors.map { ($0.position.line, $0.name) }, uniquingKeysWith: { first, _ in first })
    self.selectedLine = selectedLine
    needsDisplay = true
  }

  func setLayout(_ value: EditorWrapLayout) {
    layout = value
    needsDisplay = true
  }

  override func draw(_ rect: NSRect) {
    super.draw(rect)
    NSColor(MacTheme.face).setFill()
    rect.fill()
    guard let textView, let scrollView, layout.lineCount > 0 else { return }
    let offset = scrollView.contentView.bounds.minY
    let inset = textView.textContainerInset.height
    let font = MacTheme.monoNSFont(MacTheme.TypeScale.small)
    let attributes: [NSAttributedString.Key: Any] = [
      .font: font, .foregroundColor: NSColor(MacTheme.muted),
    ]
    let topRow = Int(max(0, offset - inset) / Self.rowHeight)
    let slack = layout.columns == nil ? 1 : Self.wrapSlackRows
    for index in layout.line(atRow: max(0, topRow - slack))..<layout.lineCount {
      let line = startLine + UInt32(clamping: index)
      let y = inset + lineTop(index, in: textView) - offset
      if y > rect.maxY { break }
      let height = CGFloat(layout.rows(ofLine: index)) * Self.rowHeight
      guard y + height >= rect.minY else { continue }
      if selectedLine == line {
        NSColor(MacTheme.well).setFill()
        NSRect(x: 0, y: y, width: Self.width, height: height).fill()
      }
      drawMarkers(diffMarkers[line] ?? [], y: y, height: height)
      drawAgentFacts(line: line, y: y, height: height)
      let label = "\(UInt64(line) + 1)" as NSString
      let size = label.size(withAttributes: attributes)
      label.draw(
        at: NSPoint(x: Self.width - size.width - 10, y: y + 2), withAttributes: attributes)
    }
  }

  /// Where page line `index` starts below the text container's top.
  private func lineTop(_ index: Int, in textView: NSTextView) -> CGFloat {
    let counted = CGFloat(layout.rowStarts[index]) * Self.rowHeight
    guard layout.columns != nil, let manager = textView.layoutManager,
      let length = textView.textStorage?.length
    else { return counted }
    let start = layout.lineStarts[index]
    guard start < length else {
      let extra = manager.extraLineFragmentRect
      return extra.isEmpty ? counted : extra.minY
    }
    let glyph = manager.glyphIndexForCharacter(at: start)
    return manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).minY
  }

  private func drawAgentFacts(line: UInt32, y: CGFloat, height: CGFloat) {
    if agentReviewLines.contains(line) {
      NSColor(MacTheme.agentBlue).setFill()
      NSRect(x: 4, y: y, width: 3, height: height).fill()
    }
    guard let name = agentCursors[line] else { return }
    NSColor(MacTheme.accent).setFill()
    NSBezierPath(ovalIn: NSRect(x: 9, y: y + 7, width: 6, height: 6)).fill()
    toolTip = "\(name) is editing line \(UInt64(line) + 1)"
  }

  private func drawMarkers(_ markers: Set<EditorDiffMarker>, y: CGFloat, height: CGFloat) {
    let ordered = markers.sorted { $0.rawValue < $1.rawValue }
    guard !ordered.isEmpty else { return }
    let share = height / CGFloat(ordered.count)
    for (index, marker) in ordered.enumerated() {
      markerColor(marker).setFill()
      NSRect(x: 0, y: y + CGFloat(index) * share, width: 3, height: share).fill()
    }
  }

  private func markerColor(_ marker: EditorDiffMarker) -> NSColor {
    switch marker {
    case .added: NSColor(MacTheme.diffAdded)
    case .modified: NSColor(MacTheme.warning)
    case .deleted: NSColor(MacTheme.danger)
    }
  }
}
