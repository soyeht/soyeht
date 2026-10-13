import Foundation
import SoyehtProtocol

extension EngineClient {
  func applyTaskSnapshot(
    _ sections: [String: [JsonValue]], panes: [Pane]
  ) throws {
    tasks = try (sections["tasks"] ?? []).map { try $0.decoded(TaskRecord.self) }
    let taskIds = Set(tasks.map(\.taskId))
    taskDetails = taskDetails.filter { taskIds.contains($0.key) }

    let paneWorkspaces = Dictionary(uniqueKeysWithValues: panes.map { ($0.paneId, $0.workspaceId) })
    var attention: [String: [AttentionItem]] = [:]
    for value in sections["attention"] ?? [] {
      let item = try value.decoded(AttentionItem.self)
      guard let workspaceId = paneWorkspaces[item.paneId] else { continue }
      attention[workspaceId, default: []].append(item)
    }
    attentionByWorkspace = attention

    var loops: [String: LoopConfig] = [:]
    for value in sections["loops"] ?? [] {
      guard let paneId = value["paneId"].string else { throw ClientFailure.invalidSnapshot }
      loops[paneId] = try value["config"].decoded(LoopConfig.self)
    }
    paneLoops = loops
  }

  func applyTaskEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .taskCreated:
      upsertTask(try params.decoded(TaskCreatedEvent.self).task)
    case .taskPhraseChanged:
      let event = try params.decoded(TaskPhraseChangedEvent.self)
      updateTask(event.taskId) { $0.phrase = event.phrase }
    case .taskCommented:
      let event = try params.decoded(TaskCommentedEvent.self)
      updateDetail(event.taskId) { detail in
        detail.comments.removeAll { $0.commentId == event.comment.commentId }
        detail.comments.append(event.comment)
      }
    case .taskSubitemChanged:
      let event = try params.decoded(TaskSubitemChangedEvent.self)
      updateDetail(event.taskId) { detail in
        detail.subitems.removeAll { $0.subitemId == event.subitem.subitemId }
        detail.subitems.append(event.subitem)
      }
    case .taskMoved:
      let event = try params.decoded(TaskMovedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskClaimed:
      let event = try params.decoded(TaskClaimedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskReleased:
      let event = try params.decoded(TaskReleasedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskBlocked:
      let event = try params.decoded(TaskBlockedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskUnblocked:
      let event = try params.decoded(TaskUnblockedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskUpdated:
      let event = try params.decoded(TaskUpdatedEvent.self)
      scheduleTaskRefresh(workspaceId: event.workspaceId, taskId: event.taskId)
    case .taskAttention:
      _ = try params.decoded(TaskAttentionEvent.self)
    default: break
    }
  }

  func applyAttentionEvent(_ params: JsonValue) throws {
    let event = try params.decoded(AttentionChangedEvent.self)
    attentionByWorkspace[event.workspaceId] = event.items
  }

  func applyLoopEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .loopTick:
      let event = try params.decoded(LoopTickEvent.self)
      guard var config = paneLoops[event.paneId] else { return }
      config.rounds = event.round
      paneLoops[event.paneId] = config
    case .loopStopped:
      let event = try params.decoded(LoopStoppedEvent.self)
      guard var config = paneLoops[event.paneId] else { return }
      config.enabled = false
      paneLoops[event.paneId] = config
    default: break
    }
  }

  public func tasks(in workspaceId: String) -> [TaskRecord] {
    tasks.filter { $0.workspaceId == workspaceId }
  }

  public func attention(in workspaceId: String) -> [AttentionItem] {
    attentionByWorkspace[workspaceId] ?? []
  }

  public func refreshTasks(_ workspaceId: String) {
    guard connected else { return }
    perform { try await self.loadTasks(workspaceId) }
  }

  public func refreshTask(workspaceId: String, taskId: String) {
    scheduleTaskRefresh(workspaceId: workspaceId, taskId: taskId)
  }

  public func createTask(
    workspaceId: String, title: String, description: String, howToVerify: String?,
    mustNotChange: String, executor: TaskExecutor?, priority: TaskPriority,
    createWorktree: Bool, dependencies: [String],
    dispatch: Bool
  ) async throws -> TaskCreateResult {
    let result = try await mutation(
      .taskCreate,
      TaskCreateParams(
        workspaceId: workspaceId, title: title, description: description,
        howToVerify: howToVerify, mustNotChange: mustNotChange, executor: executor,
        priority: priority, createWorktree: createWorktree,
        dependencies: dependencies, dispatch: dispatch,
        idempotencyKey: UUID().uuidString)
    ).decoded(TaskCreateResult.self)
    upsertTask(result.task)
    if let paneId = result.paneId { select(paneId) }
    return result
  }

  public func moveTask(_ task: TaskRecord, to state: TaskState, reason: String) {
    perform {
      let result = try await self.mutation(
        .taskMove,
        TaskMoveParams(
          workspaceId: task.workspaceId, taskId: task.taskId, state: state,
          reason: reason, idempotencyKey: UUID().uuidString)
      ).decoded(TaskMoveResult.self)
      self.upsertTask(result.task)
      try await self.loadTask(workspaceId: task.workspaceId, taskId: task.taskId)
    }
  }

  public func dispatchTask(_ task: TaskRecord) {
    perform {
      let preset = task.executor ?? TaskExecutor(workspaceDefault: true)
      let result = try await self.mutation(
        .taskDispatch,
        TaskDispatchParams(
          workspaceId: task.workspaceId, taskId: task.taskId, preset: preset,
          idempotencyKey: UUID().uuidString),
        timeoutSeconds: 95
      ).decoded(TaskDispatchResult.self)
      self.upsertTask(result.task)
      self.select(result.paneId)
    }
  }

  public func setLoop(
    paneId: String, prompt: String, interval: UInt32, maxRounds: UInt32,
    stopWhen: LoopStopWhen, enabled: Bool
  ) {
    perform {
      let result = try await self.mutation(
        .loopSet,
        LoopSetParams(
          paneId: paneId, prompt: prompt, interval: interval, maxRounds: maxRounds,
          stopWhen: stopWhen, enabled: enabled, idempotencyKey: UUID().uuidString)
      ).decoded(LoopSetResult.self)
      self.paneLoops[paneId] = result.config
    }
  }

  public func setPaneRole(_ paneId: String, role: AgentPaneRole) {
    perform {
      _ = try await self.mutation(
        .paneRoleSet,
        PaneRoleSetParams(
          paneId: paneId, role: role, idempotencyKey: UUID().uuidString))
    }
  }

  private func scheduleTaskRefresh(workspaceId: String, taskId: String) {
    guard connected else { return }
    perform { try await self.loadTask(workspaceId: workspaceId, taskId: taskId) }
  }

  private func loadTask(workspaceId: String, taskId: String) async throws {
    let result = try await request(
      .taskShow, .encoded(TaskShowParams(workspaceId: workspaceId, taskId: taskId))
    ).decoded(TaskShowResult.self)
    upsertTask(result.task)
    taskDetails[taskId] = result
  }

  private func loadTasks(_ workspaceId: String) async throws {
    var cursor: String?
    var loaded: [TaskRecord] = []
    repeat {
      let result = try await request(
        .taskList,
        .encoded(TaskListParams(workspaceId: workspaceId, limit: 100, cursor: cursor))
      ).decoded(TaskListResult.self)
      loaded.append(contentsOf: result.items)
      cursor = result.nextCursor
    } while cursor != nil
    tasks.removeAll { $0.workspaceId == workspaceId }
    tasks.append(contentsOf: loaded)
  }

  private func upsertTask(_ task: TaskRecord) {
    tasks.removeAll { $0.taskId == task.taskId }
    tasks.append(task)
    if var detail = taskDetails[task.taskId] {
      detail.task = task
      taskDetails[task.taskId] = detail
    }
  }

  private func updateTask(_ taskId: String, change: (inout TaskRecord) -> Void) {
    guard let index = tasks.firstIndex(where: { $0.taskId == taskId }) else { return }
    change(&tasks[index])
    if var detail = taskDetails[taskId] {
      detail.task = tasks[index]
      taskDetails[taskId] = detail
    }
  }

  private func updateDetail(_ taskId: String, change: (inout TaskShowResult) -> Void) {
    guard var detail = taskDetails[taskId] else { return }
    change(&detail)
    taskDetails[taskId] = detail
  }
}
