import Foundation
import Observation
import SoyehtProtocol

/// What the engine last said about the trust of each open workspace's folder, and the prompt
/// rules the clients share: an undecided folder is asked about once per app session, and
/// "Not now" or a dismissed sheet is not asked again until the app starts over. It owns no
/// connection: `EngineClient` hands it the engine's answers and the request to answer with.
@MainActor @Observable
public final class ProjectTrustReplica: ClientReplica {
  /// The trust of each workspace the engine listed; a workspace it did not list is absent.
  public private(set) var entries: [String: ProjectTrust] = [:]
  /// The workspace whose sheet the person asked to see again (the editor's restricted marker).
  public private(set) var requested: String?
  /// Workspaces whose prompt was dismissed in this app session. A disconnect keeps them: the
  /// answer is owed to the person, not to the connection.
  @ObservationIgnored private var deferred: Set<String> = []

  /// Replaces every entry with the engine's list.
  func load(_ items: [ProjectTrustEntry]) {
    entries = Dictionary(items.map { ($0.workspaceId, $0.trust) }) { _, last in last }
  }

  func apply(_ event: ProjectTrustChangedEvent) {
    apply(workspaceIds: event.workspaceIds, trust: event.trust)
  }

  func apply(_ result: ProjectTrustSetResult) {
    apply(workspaceIds: result.workspaceIds, trust: result.trust)
  }

  private func apply(workspaceIds: [String], trust: ProjectTrust) {
    for workspaceId in workspaceIds { entries[workspaceId] = trust }
    if let requested, workspaceIds.contains(requested) { self.requested = nil }
  }

  func forget(_ workspaceId: String) {
    entries[workspaceId] = nil
    deferred.remove(workspaceId)
    if requested == workspaceId { requested = nil }
  }

  func reset() {
    entries = [:]
    requested = nil
  }

  func cancelInFlight() {}

  /// What the engine says about `workspaceId`; `nil` when it said nothing (an engine that does
  /// not know the question).
  public func trust(for workspaceId: String) -> ProjectTrust? {
    entries[workspaceId]
  }

  /// Whether the folder runs without code execution: undecided behaves as restricted. A folder
  /// the engine did not list is not claimed to be restricted.
  public func isRestricted(_ workspaceId: String) -> Bool {
    switch entries[workspaceId] {
    case .undecided, .restricted: true
    case .trusted, nil: false
    }
  }

  /// Whether the person has yet to be asked: the folder is undecided and was not put off in this
  /// app session.
  public func needsDecision(_ workspaceId: String) -> Bool {
    entries[workspaceId] == .undecided && !deferred.contains(workspaceId)
  }

  /// The workspace to ask about while `selected` is the one on screen: the one the person asked
  /// to review, else `selected` when it needs a decision.
  public func promptedWorkspace(selected: String?) -> String? {
    if let requested { return requested }
    guard let selected, needsDecision(selected) else { return nil }
    return selected
  }

  /// Shows the sheet of `workspaceId` again, whatever was answered before.
  public func requestReview(_ workspaceId: String) {
    deferred.remove(workspaceId)
    requested = workspaceId
  }

  /// "Not now": the sheet closes and the folder is not asked about again until relaunch.
  public func putOff(_ workspaceId: String) {
    deferred.insert(workspaceId)
    if requested == workspaceId { requested = nil }
  }
}
