import Foundation

/// Where an agent transcript was left. A pane's view is unmounted when its workspace is not
/// shown, so the place lives with the pane's replica: coming back restores it, at the
/// bottom for a person who was following the agent or at the same row for one who had
/// scrolled up. A pane never seen starts at the bottom, as after a relaunch.
///
/// A reference, read and written by the transcript view on the main actor only.
public final class AgentTranscriptScroll: @unchecked Sendable {
  public enum Place: Equatable, Sendable {
    case bottom
    case row(String)
  }

  /// How many jumps a restoration may take, a moment apart.
  static let restorePasses = 3
  static let restorePassMs = 100

  public private(set) var place: Place = .bottom

  public init() {}

  /// Records the row at the viewport's bottom edge, or the bottom marker when the newest
  /// message is in view. `nil` means the scroll view has not reported yet.
  public func record(bottomVisibleId: String?) {
    guard let bottomVisibleId else { return }
    place =
      bottomVisibleId == AgentConversationFollow.bottomAnchor
      ? .bottom : .row(bottomVisibleId)
  }

  /// Where to scroll when the pane returns. A row that is no longer in the transcript
  /// (older messages not loaded) falls back to the bottom.
  public func restoration(rowIds: [String]) -> Place {
    if case .row(let id) = place, !rowIds.contains(id) { return .bottom }
    return place
  }
}
