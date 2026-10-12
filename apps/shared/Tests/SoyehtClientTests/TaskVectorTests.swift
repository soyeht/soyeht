import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/tasks.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("tasks") }

private func records(_ all: JsonValue) throws -> [String: TaskRecord] {
  var found: [String: TaskRecord] = [:]
  for item in all["tasks"].array ?? [] {
    let record = try item.decoded(TaskRecord.self)
    found[record.taskId] = record
  }
  return found
}

private func pick(_ ids: JsonValue, from all: [String: TaskRecord]) -> [TaskRecord] {
  (ids.array ?? []).compactMap { $0.string.flatMap { all[$0] } }
}

private func state(_ name: String?) throws -> TaskState {
  try JsonValue.string(name ?? "").decoded(TaskState.self)
}

@Test func theBoardAndTheListSortTasksTheWayTheVectorSays() throws {
  let all = try vector()
  let held = try records(all)
  for item in ContractVector.cases(all, "stateTitles") {
    let value = try state(item["state"].string)
    #expect(TaskPresentation.title(value) == item["title"].string)
    #expect(TaskPresentation.role(value).rawValue == item["role"].string)
  }
  for item in ContractVector.cases(all, "columns") {
    let tasks = pick(item["taskIds"], from: held)
    #expect(
      try TaskPresentation.boardStates(tasks)
        == (item["board"].array ?? []).map { try state($0.string) },
      "\(item["name"].string ?? "")")
    #expect(
      try TaskPresentation.listStates(tasks)
        == (item["list"].array ?? []).map { try state($0.string) })
  }
  let order = all["listOrder"]
  let sorted = pick(order["taskIds"], from: held).sorted(by: TaskPresentation.listPrecedes)
  #expect(sorted.map(\.taskId) == (order["sorted"].array ?? []).compactMap(\.string))
  for item in ContractVector.cases(all, "priorityTitles") {
    let priority = try item["priority"].decoded(TaskPriority.self)
    #expect(TaskPresentation.title(priority) == item["title"].string)
  }
  for item in ContractVector.cases(all, "badges") {
    let badge = try item["badge"].decoded(TaskReviewBadge.self)
    #expect(TaskPresentation.title(badge) == item["title"].string)
    #expect(TaskPresentation.role(badge).rawValue == item["role"].string)
  }
  for item in ContractVector.cases(all, "blockingTitles") {
    #expect(
      TaskPresentation.title(try item["kind"].decoded(BlockingKind.self)) == item["title"].string)
  }
  for item in ContractVector.cases(all, "attemptTitles") {
    #expect(
      TaskPresentation.title(try item["result"].decoded(AttemptResult.self)) == item["title"].string
    )
  }
  for item in ContractVector.cases(all, "ownerPlates") {
    #expect(TaskPresentation.ownerPlate(item["name"].string ?? "").rawValue == item["plate"].string)
  }
}

@Test func aCardSaysWhereATaskStandsAndTheSummaryCountsThem() throws {
  let all = try vector()
  let held = try records(all)
  for item in ContractVector.cases(all, "statusLines") {
    let task = try #require(held[item["taskId"].string ?? ""])
    #expect(TaskPresentation.statusLine(task) == item["line"].string)
    #expect(TaskPresentation.indicator(task).rawValue == item["indicator"].string)
  }
  for item in ContractVector.cases(all, "summary") {
    #expect(
      TaskPresentation.summary(
        tasks: pick(item["taskIds"], from: held), attention: item["attention"].int ?? 0)
        == item["text"].string)
  }
  let dispatch = all["dispatch"]
  let ready = TaskPresentation.readyToDispatch(pick(dispatch["taskIds"], from: held))
  #expect(ready.map(\.taskId) == (dispatch["ready"].array ?? []).compactMap(\.string))
  for task in pick(dispatch["taskIds"], from: held) {
    #expect(TaskPresentation.dispatchLabel(task) == dispatch["labels"][task.taskId].string)
  }
  #expect(TaskPresentation.noTaskReady == dispatch["none"].string)
  #expect(TaskPresentation.emptyTitle == all["words"]["emptyTitle"].string)
  #expect(TaskPresentation.emptyDetail == all["words"]["emptyDetail"].string)
}

@Test func theDetailNamesTheRunnerTheSubitemsTheAttemptsAndTheTimeline() throws {
  let all = try vector()
  let held = try records(all)
  for item in ContractVector.cases(all, "runners") {
    var task = try #require(held["WS-6"])
    task.executor = try? item["executor"].decoded(TaskExecutor.self)
    #expect(TaskPresentation.runnerName(task) == item["name"].string)
  }
  for item in ContractVector.cases(all, "subitems") {
    let values = try (item["states"].array ?? []).enumerated().map { index, state in
      try JsonValue.object([
        "subitemId": .string("s\(index)"), "taskId": .string("WS-5"), "text": .string("t"),
        "state": state,
      ]).decoded(Subitem.self)
    }
    #expect(TaskPresentation.subitemsTitle(values) == item["title"].string)
  }
  for item in ContractVector.cases(all, "attempts") {
    let attempt = try item["attempt"].decoded(Attempt.self)
    #expect(TaskPresentation.attemptLine(attempt) == item["line"].string)
    #expect(TaskPresentation.attemptOutcome(attempt) == item["outcome"].string)
  }
  let timeline = all["timeline"]
  let detail = try JsonValue.object([
    "task": try JsonValue.encoded(try #require(held["WS-5"])), "subitems": .array([]),
    "attempts": .array([]), "history": timeline["history"], "comments": timeline["comments"],
  ]).decoded(TaskShowResult.self)
  let entries = TaskPresentation.timeline(detail)
  let expected = timeline["entries"].array ?? []
  #expect(entries.count == expected.count)
  for (entry, wanted) in zip(entries, expected) {
    #expect(entry.id == wanted["id"].string)
    #expect(entry.author == wanted["author"].string)
    #expect(entry.text == wanted["text"].string)
    #expect(entry.at == wanted["at"].string)
  }
  #expect(
    TaskPresentation.driftFindings(detail.comments)
      == (timeline["findings"].array ?? []).compactMap(\.string))
  let declarations = all["declarations"]
  let change = try declarations["contractChange"].decoded(ContractChange.self)
  #expect(TaskPresentation.contractChange(change) == declarations["contractChange"]["text"].string)
  let edit = try declarations["outOfScope"].decoded(OutOfScopeEdit.self)
  #expect(TaskPresentation.outOfScopeEdit(edit) == declarations["outOfScope"]["text"].string)
}

@Test func theNewTaskFormSaysWhatIsMissing() throws {
  let form = try vector()["newTask"]
  for item in ContractVector.cases(form, "missing") {
    #expect(
      TaskPresentation.missingRequiredFields(
        title: item["title"].string ?? "", description: item["description"].string ?? "",
        mustNotChange: item["mustNotChange"].string ?? "")
        == (item["missing"].array ?? []).compactMap(\.string))
  }
  for item in ContractVector.cases(form, "hints") {
    #expect(
      TaskPresentation.actionHint(
        missing: (item["missing"].array ?? []).compactMap(\.string),
        workspaceDefault: item["workspaceDefault"].bool == true,
        harness: item["harness"].string ?? "",
        shortcuts: item["shortcuts"].string ?? "") == item["text"].string)
  }
  for item in ContractVector.cases(form, "dependencies") {
    let ids = Set((item["dependencies"].array ?? []).compactMap(\.string))
    #expect(TaskPresentation.dependencyTitle(ids) == item["text"].string)
  }
  for item in ContractVector.cases(form, "runnerTitles") {
    #expect(
      TaskPresentation.runnerTitle(
        workspaceDefault: item["workspaceDefault"].bool == true,
        harnessName: item["harnessName"].string,
        modelName: item["modelName"].string, effortName: item["effortName"].string)
        == item["text"].string)
  }
}
