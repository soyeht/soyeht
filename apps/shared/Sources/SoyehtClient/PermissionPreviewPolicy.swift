import SoyehtProtocol

public struct PermissionPreview: Sendable {
  public let removed: [String]
  public let added: [String]
  public let unavailable: Bool
}

public enum PermissionPreviewPolicy {
  public static func preview(_ change: PermissionFileChange) -> PermissionPreview {
    let before = lines(change.oldText ?? "")
    let after = lines(change.newText)
    guard before.count + after.count <= ProtocolLimits.permissionPreviewLines else {
      return PermissionPreview(removed: [], added: [], unavailable: true)
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
    return PermissionPreview(removed: removed, added: added, unavailable: false)
  }

  private static func lines(_ text: String) -> [String] {
    guard !text.isEmpty else { return [] }
    var result = text.components(separatedBy: "\n")
    if result.last == "" { result.removeLast() }
    return result
  }
}
