import Foundation
import Observation
import SoyehtProtocol

public enum PaneLayoutControllerError: Error, Equatable, LocalizedError, Sendable {
  case missingWorkspace(String)
  case missingTree(String)
  case draftNotStarted(String)

  public var errorDescription: String? {
    switch self {
    case .missingWorkspace(let workspaceId):
      "Pane layout is not loaded for workspace \(workspaceId)."
    case .missingTree(let workspaceId):
      "Pane layout has no tree for workspace \(workspaceId)."
    case .draftNotStarted(let workspaceId):
      "Pane layout has no active draft for workspace \(workspaceId)."
    }
  }
}

public struct PaneLayoutCommitFailure: Equatable, Sendable {
  public static let userMessage =
    "Couldn't save the layout. The last saved arrangement is back."

  public var workspaceId: String
  public var message: String

  public init(workspaceId: String, message: String) {
    self.workspaceId = workspaceId
    self.message = message
  }
}

public struct PaneLayoutWorkspacePresentation: Equatable, Sendable {
  public var confirmed: PaneLayoutState
  public var displayed: PaneLayoutState
  public var draftTree: PaneLayoutTree?
  public var pendingCommitCount: Int
  public var isPersisting: Bool

  public init(
    confirmed: PaneLayoutState, displayed: PaneLayoutState, draftTree: PaneLayoutTree?,
    pendingCommitCount: Int, isPersisting: Bool
  ) {
    self.confirmed = confirmed
    self.displayed = displayed
    self.draftTree = draftTree
    self.pendingCommitCount = pendingCommitCount
    self.isPersisting = isPersisting
  }
}

public struct PaneLayoutReconciliation: Equatable, Sendable {
  public var state: PaneLayoutState
  public var insertedPaneIds: [String]

  public init(state: PaneLayoutState, insertedPaneIds: [String]) {
    self.state = state
    self.insertedPaneIds = insertedPaneIds
  }
}

/// Owns the confirmed, optimistic, and gesture-draft layouts for every workspace.
///
/// Commits are persisted serially per workspace. A response confirms only its own queued state;
/// a newer queued state or active gesture remains displayed. A failed commit drops dependent
/// queued states and returns the workspace to the latest confirmed state.
@MainActor @Observable
public final class PaneLayoutController {
  public typealias Persist = @MainActor (String, PaneLayoutState) async throws -> PaneLayoutState

  public private(set) var failure: PaneLayoutCommitFailure?

  @ObservationIgnored private let persist: Persist
  private var entries: [String: Entry] = [:]
  @ObservationIgnored private var workers: [String: Worker] = [:]
  @ObservationIgnored private var nextCommitId: UInt64 = 0
  @ObservationIgnored private var nextWorkerId: UInt64 = 0

  public init(persist: @escaping Persist) {
    self.persist = persist
  }

  public func synchronize(workspaceId: String, confirmed state: PaneLayoutState) {
    if var entry = entries[workspaceId] {
      entry.confirmed = state
      entries[workspaceId] = entry
    } else {
      entries[workspaceId] = Entry(confirmed: state)
    }
  }

  public func remove(workspaceId: String) {
    workers.removeValue(forKey: workspaceId)?.task.cancel()
    entries.removeValue(forKey: workspaceId)
    if failure?.workspaceId == workspaceId { failure = nil }
  }

  public func retainOnly(workspaceIds: Set<String>) {
    for workspaceId in Array(entries.keys) where !workspaceIds.contains(workspaceId) {
      remove(workspaceId: workspaceId)
    }
  }

  public func removeAll() {
    for worker in workers.values { worker.task.cancel() }
    workers.removeAll()
    entries.removeAll()
    failure = nil
  }

  public func presentation(
    for workspaceId: String
  ) -> PaneLayoutWorkspacePresentation? {
    guard let entry = entries[workspaceId] else { return nil }
    return presentation(for: workspaceId, entry: entry)
  }

  /// Reconciles the displayed layout with live panes without treating the repair as confirmed.
  /// This keeps a pane created before a failed layout commit visible to the caller.
  public func reconciledPresentation(
    for workspaceId: String, livePanes: [Pane], sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutReconciliation {
    guard let displayed = presentation(for: workspaceId)?.displayed else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    var reconciled = displayed
    let inserted = try reconciled.reconcile(livePanes: livePanes, sizing: sizing)
    return PaneLayoutReconciliation(state: reconciled, insertedPaneIds: inserted)
  }

  @discardableResult
  public func beginDraft(workspaceId: String) throws -> PaneLayoutTree {
    guard var entry = entries[workspaceId] else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    guard let tree = presentedState(entry).tree else {
      throw PaneLayoutControllerError.missingTree(workspaceId)
    }
    entry.draftTree = tree
    entries[workspaceId] = entry
    return tree
  }

  /// Starts a gesture draft from the tree on screen. A workspace in automatic mode has no
  /// stored tree until its first arrangement, so the draft starts from the live panes reconciled
  /// the way the view shows them; the repair is not confirmed until the gesture commits.
  @discardableResult
  public func beginDraft(
    workspaceId: String, livePanes: [Pane], sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    guard let entry = entries[workspaceId] else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    if presentedState(entry).tree != nil { return try beginDraft(workspaceId: workspaceId) }
    guard
      let tree = try reconciledPresentation(
        for: workspaceId, livePanes: livePanes, sizing: sizing
      ).state.tree
    else { throw PaneLayoutControllerError.missingTree(workspaceId) }
    var started = entry
    started.draftTree = tree
    entries[workspaceId] = started
    return tree
  }

  public func updateDraft(workspaceId: String, tree: PaneLayoutTree) throws {
    guard var entry = entries[workspaceId] else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    guard entry.draftTree != nil else {
      throw PaneLayoutControllerError.draftNotStarted(workspaceId)
    }
    entry.draftTree = tree
    entries[workspaceId] = entry
  }

  public func cancelDraft(workspaceId: String) {
    guard var entry = entries[workspaceId] else { return }
    entry.draftTree = nil
    entries[workspaceId] = entry
  }

  public func commitDraft(workspaceId: String) throws {
    guard var entry = entries[workspaceId] else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    guard let tree = entry.draftTree else {
      throw PaneLayoutControllerError.draftNotStarted(workspaceId)
    }
    entry.draftTree = nil
    enqueue(tree: tree, workspaceId: workspaceId, entry: &entry)
    entries[workspaceId] = entry
    startWorkerIfNeeded(workspaceId: workspaceId)
  }

  /// Commits a complete tree immediately. Split completion and divider double-click use this
  /// path; drag and live resize use `beginDraft`, `updateDraft`, and `commitDraft`.
  public func commit(tree: PaneLayoutTree, workspaceId: String) throws {
    guard var entry = entries[workspaceId] else {
      throw PaneLayoutControllerError.missingWorkspace(workspaceId)
    }
    entry.draftTree = nil
    enqueue(tree: tree, workspaceId: workspaceId, entry: &entry)
    entries[workspaceId] = entry
    startWorkerIfNeeded(workspaceId: workspaceId)
  }

  public func clearFailure() {
    failure = nil
  }

  private func presentation(
    for workspaceId: String, entry: Entry
  ) -> PaneLayoutWorkspacePresentation {
    PaneLayoutWorkspacePresentation(
      confirmed: entry.confirmed,
      displayed: presentedState(entry),
      draftTree: entry.draftTree,
      pendingCommitCount: entry.queue.count,
      isPersisting: workers[workspaceId] != nil)
  }

  private func presentedState(_ entry: Entry) -> PaneLayoutState {
    var state = entry.queue.last?.state ?? entry.confirmed
    if let draftTree = entry.draftTree { state.tree = draftTree }
    return state
  }

  private func enqueue(tree: PaneLayoutTree, workspaceId: String, entry: inout Entry) {
    var state = entry.queue.last?.state ?? entry.confirmed
    state.recordUserArrangement(tree)
    nextCommitId &+= 1
    entry.queue.append(PendingCommit(id: nextCommitId, state: state))
    if failure?.workspaceId == workspaceId { failure = nil }
  }

  private func startWorkerIfNeeded(workspaceId: String) {
    guard workers[workspaceId] == nil else { return }
    nextWorkerId &+= 1
    let workerId = nextWorkerId
    let task = Task { [weak self] in
      guard let self else { return }
      await self.drain(workspaceId: workspaceId, workerId: workerId)
    }
    workers[workspaceId] = Worker(id: workerId, task: task)
  }

  private func drain(workspaceId: String, workerId: UInt64) async {
    while !Task.isCancelled {
      guard let pending = entries[workspaceId]?.queue.first else { break }
      do {
        let confirmed = try await persist(workspaceId, pending.state)
        guard var entry = entries[workspaceId], entry.queue.first?.id == pending.id else {
          break
        }
        entry.confirmed = confirmed
        entry.queue.removeFirst()
        entries[workspaceId] = entry
      } catch is CancellationError {
        break
      } catch {
        guard var entry = entries[workspaceId], entry.queue.first?.id == pending.id else {
          break
        }
        entry.queue.removeAll()
        entry.draftTree = nil
        entries[workspaceId] = entry
        failure = PaneLayoutCommitFailure(
          workspaceId: workspaceId, message: error.localizedDescription)
        break
      }
    }
    if workers[workspaceId]?.id == workerId {
      workers.removeValue(forKey: workspaceId)
      if entries[workspaceId]?.queue.isEmpty == false {
        startWorkerIfNeeded(workspaceId: workspaceId)
      }
    }
  }

  private struct Entry {
    var confirmed: PaneLayoutState
    var draftTree: PaneLayoutTree?
    var queue: [PendingCommit] = []
  }

  private struct PendingCommit {
    var id: UInt64
    var state: PaneLayoutState
  }

  private struct Worker {
    var id: UInt64
    var task: Task<Void, Never>
  }
}
