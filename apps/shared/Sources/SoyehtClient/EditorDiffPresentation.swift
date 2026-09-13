import SoyehtProtocol

public enum EditorDiffMarker: Int, Sendable, Hashable {
  case added
  case modified
  case deleted
}

public enum EditorDiffRowKind: Sendable, Equatable {
  case context
  case added
  case modified
  case removed
}

public struct EditorDiffSide: Sendable {
  public let line: UInt32?
  public let text: String?
  public let author: FileAuthor?

  public init(line: UInt32?, text: String?, author: FileAuthor?) {
    self.line = line
    self.text = text
    self.author = author
  }
}

public struct EditorDiffRow: Identifiable, Sendable {
  public let id: String
  public let old: EditorDiffSide
  public let new: EditorDiffSide
  public let kind: EditorDiffRowKind

  public init(
    id: String, old: EditorDiffSide, new: EditorDiffSide, kind: EditorDiffRowKind
  ) {
    self.id = id
    self.old = old
    self.new = new
    self.kind = kind
  }
}

public struct EditorDiffPresentation: Sendable {
  public let rows: [EditorDiffRow]
  public let markers: [UInt32: Set<EditorDiffMarker>]
  public let blameByLine: [UInt32: FileBlame]

  public init(_ result: FileDiffResult) {
    var rows: [EditorDiffRow] = []
    var markers: [UInt32: Set<EditorDiffMarker>] = [:]
    for hunk in result.hunks {
      Self.append(hunk, to: &rows, markers: &markers)
    }
    self.rows = rows
    self.markers = markers
    blameByLine = Dictionary(
      result.blame.map { ($0.line, $0) }, uniquingKeysWith: { first, _ in first })
  }

  public func gutterMarkers(lineCount: UInt32) -> [UInt32: Set<EditorDiffMarker>] {
    guard lineCount > 0 else { return [:] }
    var visible: [UInt32: Set<EditorDiffMarker>] = [:]
    for (line, values) in markers {
      visible[min(line, lineCount - 1), default: []].formUnion(values)
    }
    return visible
  }

  private static func append(
    _ hunk: DiffHunk, to rows: inout [EditorDiffRow],
    markers: inout [UInt32: Set<EditorDiffMarker>]
  ) {
    var pending: [DiffLine] = []
    var nextNewLine = hunk.newStart
    var rowIndex = 0
    for line in hunk.lines {
      if line.kind == .context {
        appendChange(
          pending, hunkId: hunk.hunkId, index: &rowIndex, anchor: nextNewLine,
          to: &rows, markers: &markers)
        pending = []
        rows.append(contextRow(line, hunkId: hunk.hunkId, index: rowIndex))
        rowIndex += 1
        if let newLine = line.newLine { nextNewLine = following(newLine) }
      } else {
        pending.append(line)
        if let newLine = line.newLine { nextNewLine = following(newLine) }
      }
    }
    appendChange(
      pending, hunkId: hunk.hunkId, index: &rowIndex,
      anchor: changeAnchor(pending, fallback: nextNewLine), to: &rows, markers: &markers)
  }

  private static func appendChange(
    _ lines: [DiffLine], hunkId: String, index: inout Int, anchor: UInt32,
    to rows: inout [EditorDiffRow], markers: inout [UInt32: Set<EditorDiffMarker>]
  ) {
    let removed = lines.filter { $0.kind == .removed }
    let added = lines.filter { $0.kind == .added }
    for offset in 0..<max(removed.count, added.count) {
      let old = offset < removed.count ? side(removed[offset], old: true) : emptySide
      let new = offset < added.count ? side(added[offset], old: false) : emptySide
      let kind = rowKind(hasOld: old.text != nil, hasNew: new.text != nil)
      rows.append(EditorDiffRow(id: "\(hunkId):\(index)", old: old, new: new, kind: kind))
      index += 1
      mark(kind, line: new.line ?? anchor, markers: &markers)
    }
  }

  private static func contextRow(_ line: DiffLine, hunkId: String, index: Int) -> EditorDiffRow {
    EditorDiffRow(
      id: "\(hunkId):\(index)", old: side(line, old: true), new: side(line, old: false),
      kind: .context)
  }

  private static func side(_ line: DiffLine, old: Bool) -> EditorDiffSide {
    EditorDiffSide(line: old ? line.oldLine : line.newLine, text: line.text, author: line.author)
  }

  private static let emptySide = EditorDiffSide(line: nil, text: nil, author: nil)

  private static func rowKind(hasOld: Bool, hasNew: Bool) -> EditorDiffRowKind {
    if hasOld && hasNew { return .modified }
    return hasOld ? .removed : .added
  }

  private static func mark(
    _ kind: EditorDiffRowKind, line: UInt32, markers: inout [UInt32: Set<EditorDiffMarker>]
  ) {
    switch kind {
    case .added: markers[line, default: []].insert(.added)
    case .modified: markers[line, default: []].insert(.modified)
    case .removed: markers[line, default: []].insert(.deleted)
    case .context: break
    }
  }

  private static func changeAnchor(_ lines: [DiffLine], fallback: UInt32) -> UInt32 {
    lines.compactMap(\.newLine).min() ?? fallback
  }

  private static func following(_ line: UInt32) -> UInt32 {
    line == UInt32.max ? UInt32.max : line + 1
  }
}
