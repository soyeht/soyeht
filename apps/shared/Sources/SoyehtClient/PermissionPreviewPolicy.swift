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
    let (removed, added) = changedLines(from: before, to: after)
    let rows =
      (removed.map { (PermissionPreviewRow.Kind.removed, $0) }
      + added.map { (PermissionPreviewRow.Kind.added, $0) })
      .enumerated()
      .map { index, row in PermissionPreviewRow(id: index, kind: row.0, text: row.1) }
    return PermissionPreview(rows: rows, unavailable: false)
  }

  /// The lines of `before` that `after` does not keep, and those of `after` that are new. The
  /// lines both start or end with are kept; of the rest, a longest common run of lines is kept,
  /// and when several are as long, the one found by taking the earliest line of `before` out
  /// first. The desktop client follows the same rule (`contracts/vectors/permission-preview.json`),
  /// because the standard library's diff does not promise which of several equally short
  /// differences it returns.
  static func changedLines(from before: [String], to after: [String]) -> (
    removed: [String], added: [String]
  ) {
    let shortest = min(before.count, after.count)
    var head = 0
    while head < shortest, before[head] == after[head] { head += 1 }
    var tail = 0
    while tail < shortest - head, before[before.count - 1 - tail] == after[after.count - 1 - tail] {
      tail += 1
    }
    let old = Array(before[head..<(before.count - tail)])
    let new = Array(after[head..<(after.count - tail)])
    // `kept[i][j]`: how many lines the tails `old[i...]` and `new[j...]` share in order.
    var kept = Array(
      repeating: Array(repeating: 0, count: new.count + 1), count: old.count + 1)
    for i in stride(from: old.count - 1, through: 0, by: -1) {
      for j in stride(from: new.count - 1, through: 0, by: -1) {
        kept[i][j] =
          old[i] == new[j] ? kept[i + 1][j + 1] + 1 : max(kept[i + 1][j], kept[i][j + 1])
      }
    }
    var removed: [String] = []
    var added: [String] = []
    var i = 0
    var j = 0
    while i < old.count, j < new.count {
      if old[i] == new[j] {
        i += 1
        j += 1
      } else if kept[i + 1][j] >= kept[i][j + 1] {
        removed.append(old[i])
        i += 1
      } else {
        added.append(new[j])
        j += 1
      }
    }
    return (removed + old[i...], added + new[j...])
  }

  private static func lines(_ text: String) -> [String] {
    guard !text.isEmpty else { return [] }
    var result = text.components(separatedBy: "\n")
    if result.last == "" { result.removeLast() }
    return result
  }
}
