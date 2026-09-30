import Foundation
import Observation
import SoyehtProtocol

public struct AgentAttentionRequest: Equatable, Hashable, Sendable {
  public let requestId: String
  public let paneId: String
  public let workspaceId: String
  public let paneName: String
  public let harness: String?
  public let generation: UInt32

  public var identity: String { "\(paneId):\(generation):\(requestId)" }
}

public struct AgentAttentionSummary: Equatable, Sendable {
  public static let empty = AgentAttentionSummary(
    requests: [], workingPaneIdsByWorkspace: [:])

  public let requests: [AgentAttentionRequest]
  public let workingPaneIdsByWorkspace: [String: Set<String>]

  public var totalWaitingCount: Int { requests.count }

  public var hasWorkingAgent: Bool {
    workingPaneIdsByWorkspace.values.contains { !$0.isEmpty }
  }

  public func waitingCount(in workspaceId: String) -> Int {
    requests.count { $0.workspaceId == workspaceId }
  }

  public func hasWorkingAgent(in workspaceId: String) -> Bool {
    !(workingPaneIdsByWorkspace[workspaceId]?.isEmpty ?? true)
  }

}

public enum AgentAttentionPolicy {
  public static func waitingLabel(count: Int) -> String {
    "\(count) \(count == 1 ? "request" : "requests") waiting"
  }

  public static func summarize(
    panes: [Pane], agentPanes: [String: AgentPaneView]
  ) -> AgentAttentionSummary {
    var requests: [AgentAttentionRequest] = []
    var working: [String: Set<String>] = [:]
    for pane in panes where pane.mode == .agent {
      let view = agentPanes[pane.paneId]
      if pane.state == .working {
        working[pane.workspaceId, default: []].insert(pane.paneId)
      }
      if let permission = view?.permission {
        requests.append(
          AgentAttentionRequest(
            requestId: permission.requestId, paneId: pane.paneId,
            workspaceId: pane.workspaceId, paneName: pane.name,
            harness: pane.agent?.harness, generation: permission.generation))
      }
      if let elicitation = view?.elicitation {
        requests.append(
          AgentAttentionRequest(
            requestId: elicitation.requestId, paneId: pane.paneId,
            workspaceId: pane.workspaceId, paneName: pane.name,
            harness: pane.agent?.harness, generation: elicitation.generation))
      }
    }
    requests.sort { $0.identity < $1.identity }
    return AgentAttentionSummary(
      requests: requests, workingPaneIdsByWorkspace: working)
  }
}

/// The first connected snapshot is existing state, not a new interruption.
/// Retaining observed IDs also prevents repeat notices after reconnect.
public struct AgentAttentionNotificationCursor: Sendable {
  private var observedRequestIds: Set<String> = []
  private var seeded = false

  public init() {}

  public mutating func newRequests(
    in summary: AgentAttentionSummary, connected: Bool
  ) -> [AgentAttentionRequest] {
    guard connected else { return [] }
    let currentIds = Set(summary.requests.map(\.identity))
    guard seeded else {
      observedRequestIds.formUnion(currentIds)
      seeded = true
      return []
    }
    let new = summary.requests.filter { !observedRequestIds.contains($0.identity) }
    observedRequestIds.formUnion(currentIds)
    return new
  }
}

@MainActor @Observable
public final class AgentAttentionNotificationSettings {
  public static let enabledKey = "soyeht.agentAttention.notifications.enabled"

  public private(set) var enabled: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsClientPreferenceStore())
  }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    enabled = preferences.bool(forKey: Self.enabledKey)
  }

  public func setEnabled(_ enabled: Bool) {
    self.enabled = enabled
    preferences.set(enabled, forKey: Self.enabledKey)
  }

  public func shouldPost(appIsActive: Bool) -> Bool {
    enabled && !appIsActive
  }
}

extension EngineClient {
  public var agentAttention: AgentAttentionSummary {
    guard connected else { return .empty }
    return AgentAttentionPolicy.summarize(panes: panes, agentPanes: agentPanes)
  }
}
