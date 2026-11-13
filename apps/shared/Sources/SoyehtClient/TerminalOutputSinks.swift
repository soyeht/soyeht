import Foundation

/// Where terminal bytes go: the one sink of the pane shown alone, and the sink each surface
/// claimed for its pane. An owner stamps every claim, so a late teardown of a replaced surface
/// cannot clear the sink that replaced it.
@MainActor
final class TerminalOutputSinks {
  private struct Target {
    var owner: ObjectIdentifier
    var sink: (Data, Bool) -> Void
  }

  private var selectedSink: ((Data, Bool) -> Void)?
  private var selectedOwner: ObjectIdentifier?
  private var targets: [String: Target] = [:]

  var count: Int { targets.count }

  /// The sink of the pane shown alone, with no owner: a direct assignment by the app.
  var selected: ((Data, Bool) -> Void)? {
    get { selectedSink }
    set {
      selectedOwner = nil
      selectedSink = newValue
    }
  }

  func claimSelected(owner: AnyObject, sink: @escaping (Data, Bool) -> Void) {
    selectedOwner = ObjectIdentifier(owner)
    selectedSink = sink
  }

  func releaseSelected(owner: AnyObject) {
    guard selectedOwner == ObjectIdentifier(owner) else { return }
    selectedOwner = nil
    selectedSink = nil
  }

  func claim(paneId: String, owner: AnyObject, sink: @escaping (Data, Bool) -> Void) {
    targets[paneId] = Target(owner: ObjectIdentifier(owner), sink: sink)
  }

  /// Whether `owner` held the pane's sink, which is now released.
  func release(paneId: String, owner: AnyObject) -> Bool {
    guard targets[paneId]?.owner == ObjectIdentifier(owner) else { return false }
    return targets.removeValue(forKey: paneId) != nil
  }

  func remove(_ paneId: String) { targets.removeValue(forKey: paneId) }

  func retainOnly(_ paneIds: Set<String>) { targets = targets.filter { paneIds.contains($0.key) } }

  func deliver(to paneId: String, bytes: Data, reset: Bool, selectedPaneId: String?) {
    targets[paneId]?.sink(bytes, reset)
    if paneId == selectedPaneId { selectedSink?(bytes, reset) }
  }
}
