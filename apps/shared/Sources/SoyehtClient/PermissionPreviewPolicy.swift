import SoyehtProtocol

public struct PermissionPreview: Sendable {
  public let rows: [PermissionPreviewRow]
  public let unavailable: Bool
}

public struct PermissionPreviewRow: Equatable, Identifiable, Sendable {
  public enum Kind: Equatable, Sendable {
    case removed
    case added
  }

  public let id: Int
  public let kind: Kind
  public let text: String
}

public enum PermissionPreviewPolicy {
  /// Lines a permission well shows before it scrolls.
  public static let visibleLineLimit = 12

  public static func lineCount(_ text: String) -> Int {
    text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline).count
  }

  /// Blank lines that give a permission well the height of its visible text.
  public static func visibleLineSpacer(lines: Int) -> String {
    Array(repeating: " ", count: min(max(lines, 1), visibleLineLimit)).joined(separator: "\n")
  }

  public static func preview(_ change: PermissionFileChange) -> PermissionPreview {
    let before = lines(change.oldText ?? "")
    let after = lines(change.newText)
    guard before.count + after.count <= ProtocolLimits.permissionPreviewLines else {
      return PermissionPreview(rows: [], unavailable: true)
    }
    let difference = after.difference(from: before)
    let removed = difference.removals.compactMap { change -> String? in
      guard case .remove(_, let line, _) = change else { return nil }
      return line
    }
    let added = difference.insertions.compactMap { change -> String? in
      guard case .insert(_, let line, _) = change else { return nil }
      return line
    }
    let rows =
      (removed.map { (PermissionPreviewRow.Kind.removed, $0) }
      + added.map { (PermissionPreviewRow.Kind.added, $0) })
      .enumerated()
      .map { index, row in PermissionPreviewRow(id: index, kind: row.0, text: row.1) }
    return PermissionPreview(rows: rows, unavailable: false)
  }

  private static func lines(_ text: String) -> [String] {
    guard !text.isEmpty else { return [] }
    var result = text.components(separatedBy: "\n")
    if result.last == "" { result.removeLast() }
    return result
  }
}
