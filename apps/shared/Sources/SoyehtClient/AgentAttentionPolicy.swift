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

  /// Whether a notification's target still exists: the request is pending in that pane and
  /// workspace.
  public func contains(workspaceId: String, paneId: String, identity: String) -> Bool {
    requests.contains {
      $0.workspaceId == workspaceId && $0.paneId == paneId && $0.identity == identity
    }
  }
}

public enum AgentAttentionPolicy {
  public static let notificationTitle = "Agent needs your input"
  static let unknownWorkspaceName = "Workspace"

  /// The second line of the notification: the pane that asks and the workspace it is in.
  public static func notificationBody(paneName: String, workspaceName: String?) -> String {
    "\(paneName) \u{b7} \(workspaceName ?? unknownWorkspaceName)"
  }

  public static func waitingLabel(count: Int) -> String {
    "\(count) \(count == 1 ? "request" : "requests") waiting"
  }

  static func summarize(
    panes: [Pane], attention: (String) -> AgentPaneAttention
  ) -> AgentAttentionSummary {
    var requests: [AgentAttentionRequest] = []
    var working: [String: Set<String>] = [:]
    for pane in panes where pane.mode == .agent {
      let asks = attention(pane.paneId)
      if pane.state == .working {
        working[pane.workspaceId, default: []].insert(pane.paneId)
      }
      if let permission = asks.permission {
        requests.append(
          AgentAttentionRequest(
            requestId: permission.requestId, paneId: pane.paneId,
            workspaceId: pane.workspaceId, paneName: pane.name,
            harness: pane.agent?.harness, generation: permission.generation))
      }
      if let elicitation = asks.elicitation {
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
    in requests: [AgentAttentionRequest], connected: Bool
  ) -> [AgentAttentionRequest] {
    guard connected else { return [] }
    let currentIds = Set(requests.map(\.identity))
    guard seeded else {
      observedRequestIds.formUnion(currentIds)
      seeded = true
      return []
    }
    let new = requests.filter { !observedRequestIds.contains($0.identity) }
    observedRequestIds.formUnion(currentIds)
    return new
  }
}

@MainActor @Observable
public final class AgentAttentionNotificationSettings {
  public static let enabledKey = "soyeht.agentAttention.notifications.enabled"
  /// On until the person turns it off (`enabledByDefault` of the shared vector).
  public static let enabledByDefault = true

  public private(set) var enabled: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsClientPreferenceStore())
  }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    enabled = preferences.storedBool(forKey: Self.enabledKey) ?? Self.enabledByDefault
  }

  public func setEnabled(_ enabled: Bool) {
    self.enabled = enabled
    preferences.set(enabled, forKey: Self.enabledKey)
  }

  public func shouldPost(appIsActive: Bool) -> Bool {
    enabled && !appIsActive
  }

  /// What the person told the system about this app's notifications.
  public enum Permission: Equatable, Sendable {
    case notDetermined
    case granted
    case denied
  }

  /// What a new request that needs the person does.
  public enum Delivery: Equatable, Sendable {
    case post
    /// The system has not been asked yet: it is asked now, when the first notification is about
    /// to be delivered, and the notification is posted if the answer is yes.
    case askThenPost
    case skip
  }

  /// The one place that decides when the system's permission question is asked: never at
  /// launch (the first run already asks about starting at login), only when a notification is
  /// about to be delivered or when the person turns notifications on.
  public func delivery(appIsActive: Bool, permission: Permission) -> Delivery {
    guard shouldPost(appIsActive: appIsActive) else { return .skip }
    return switch permission {
    case .granted: .post
    case .notDetermined: .askThenPost
    case .denied: .skip
    }
  }
}

/// What the tab of one workspace shows of its agents: how many wait for the person and whether one
/// works.
public struct WorkspaceAttention: Equatable, Sendable {
  public static let none = WorkspaceAttention(waiting: 0, working: false)

  public let waiting: Int
  public let working: Bool
}

/// One workspace's `WorkspaceAttention`, observed on its own: the tab of a workspace follows this
/// box and no other, so an agent that starts or stops in another workspace does not run it.
@MainActor @Observable
final class WorkspaceAttentionBox {
  fileprivate(set) var value = WorkspaceAttention.none
}

/// The attention summary as values of their own, each changing only when what it says changes. The
/// panes and their requests change at every turn of every agent; what the window shows of them is
/// the requests (the dock badge, the notifications), the total and whether any agent works (the
/// rail) and, for each workspace, its waiting count and whether one of its agents works (its tab).
/// Each reader follows the one value it shows, so a turn in one workspace runs the views of that
/// workspace's tab and nothing else.
@MainActor @Observable
final class AgentAttentionTracker {
  private(set) var requests: [AgentAttentionRequest] = []
  private(set) var totalWaiting = 0
  private(set) var anyWorking = false
  @ObservationIgnored private(set) var summary = AgentAttentionSummary.empty
  @ObservationIgnored private var boxes: [String: WorkspaceAttentionBox] = [:]
  @ObservationIgnored private let compute: () -> AgentAttentionSummary

  init(compute: @escaping () -> AgentAttentionSummary) {
    self.compute = compute
  }

  /// The attention of one workspace; a workspace nobody asked about is made when it is first read.
  func workspace(_ workspaceId: String) -> WorkspaceAttention { box(workspaceId).value }

  /// Makes the summary again, after what it reads changed; readers hear only of a value that
  /// is different.
  func refresh() {
    let next = compute()
    guard next != summary else { return }
    summary = next
    if next.requests != requests { requests = next.requests }
    if next.totalWaitingCount != totalWaiting { totalWaiting = next.totalWaitingCount }
    if next.hasWorkingAgent != anyWorking { anyWorking = next.hasWorkingAgent }
    let workspaceIds = Set(boxes.keys).union(next.workingPaneIdsByWorkspace.keys).union(
      next.requests.map(\.workspaceId))
    for workspaceId in workspaceIds {
      let value = attention(of: workspaceId)
      let box = box(workspaceId)
      if box.value != value { box.value = value }
    }
  }

  private func box(_ workspaceId: String) -> WorkspaceAttentionBox {
    if let box = boxes[workspaceId] { return box }
    let box = WorkspaceAttentionBox()
    box.value = attention(of: workspaceId)
    boxes[workspaceId] = box
    return box
  }

  private func attention(of workspaceId: String) -> WorkspaceAttention {
    WorkspaceAttention(
      waiting: summary.waitingCount(in: workspaceId),
      working: summary.hasWorkingAgent(in: workspaceId))
  }
}

extension EngineClient {
  /// The requests the agents have open, which the dock badge and the notifications follow.
  public var agentAttentionRequests: [AgentAttentionRequest] {
    connected ? attentionTracker.requests : []
  }

  /// How many requests wait for the person, in every workspace.
  public var agentWaitingTotal: Int { connected ? attentionTracker.totalWaiting : 0 }

  /// Whether any agent works.
  public var agentWorking: Bool { connected && attentionTracker.anyWorking }

  /// What the tab of `workspaceId` shows of its agents.
  public func agentAttention(in workspaceId: String) -> WorkspaceAttention {
    connected ? attentionTracker.workspace(workspaceId) : .none
  }

  /// Whether a notification's target still exists. For a handler to ask, not for a view to read:
  /// it follows nothing.
  public func agentAttentionContains(workspaceId: String, paneId: String, identity: String) -> Bool
  {
    connected
      && attentionTracker.summary.contains(
        workspaceId: workspaceId, paneId: paneId, identity: identity)
  }

  /// The summary follows the panes and their requests: it is made again when either changes.
  func watchAttention() {
    paneStore.didChange = { [weak self] in self?.attentionTracker.refresh() }
    agentPanes.attentionChanged = { [weak self] in self?.attentionTracker.refresh() }
  }
}
