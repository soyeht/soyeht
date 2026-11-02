import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The rules the desktop client computes too, run against the vectors both pass
/// (`contracts/vectors`): the harness picker, the usage presentation and the account.
private func vector(_ name: String) throws -> [String: Any] {
  let url = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("contracts/vectors/\(name).json")
  return try #require(
    JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
}

private func decode<T: Decodable>(_ type: T.Type, _ object: Any) throws -> T {
  try JSONDecoder().decode(
    type, from: JSONSerialization.data(withJSONObject: object, options: .fragmentsAllowed))
}

private func cases(_ vector: [String: Any], _ key: String) throws -> [[String: Any]] {
  try #require(vector[key] as? [[String: Any]])
}

private func strings(_ value: Any?) -> [String] { (value as? [String]) ?? [] }

private func catalogue(_ vector: [String: Any], _ keys: Any?) throws -> [HarnessInfo] {
  let known = try #require(vector["catalogue"] as? [String: Any])
  return try strings(keys).map { key in
    try decode(HarnessInfo.self, try #require(known[key]))
  }
}

@Test func harnessLayoutsFollowTheSharedVectors() throws {
  let vector = try vector("harness-picker")
  for entry in try cases(vector, "layouts") {
    let harnesses = try catalogue(vector, entry["harnesses"])
    let layout = HarnessSwitchModel.layout(
      harnesses, current: entry["excluding"] as? String, query: entry["query"] as? String ?? "")
    let sections = layout.sections.map { [$0.group.title, $0.items.map(\.id)] as [Any] }
    let expected = try #require(entry["sections"] as? [[Any]])
    #expect(sections.count == expected.count, "\(entry["name"] ?? "")")
    for (shown, wanted) in zip(sections, expected) {
      #expect(shown[0] as? String == wanted[0] as? String, "\(entry["name"] ?? "")")
      #expect(shown[1] as? [String] == wanted[1] as? [String], "\(entry["name"] ?? "")")
    }
    #expect(layout.more.map(\.id) == strings(entry["more"]), "\(entry["name"] ?? "")")
  }
}

@Test func harnessDefaultsStatusesAndNamesFollowTheSharedVectors() throws {
  let vector = try vector("harness-picker")
  for entry in try cases(vector, "defaults") {
    let harnesses = try catalogue(vector, entry["harnesses"])
    let chosen = HarnessSwitchModel.defaultTarget(
      in: harnesses, current: entry["excluding"] as? String,
      preferredId: entry["preferred"] as? String)
    #expect(chosen?.id == entry["expected"] as? String, "\(entry["name"] ?? "")")
  }
  let known = try #require(vector["catalogue"] as? [String: Any])
  for entry in try cases(vector, "statuses") {
    let harness = try decode(
      HarnessInfo.self, try #require(known[entry["harness"] as? String ?? ""]))
    #expect(HarnessPickerModel.statusLabel(for: harness) == entry["label"] as? String)
    #expect(HarnessPickerModel.statusReason(for: harness) == entry["reason"] as? String)
    #expect(HarnessPickerModel.pickerNotice(for: harness) == entry["notice"] as? String)
    #expect(HarnessPickerModel.isInstalled(harness) == entry["installed"] as? Bool)
  }
  for entry in try cases(vector, "reasons") {
    var tool = try #require(known["tool"] as? [String: Any])
    tool["setupReason"] = entry["setupReason"]
    let harness = try decode(HarnessInfo.self, tool)
    #expect(HarnessPickerModel.statusReason(for: harness) == entry["text"] as? String)
  }
  for pair in try #require(vector["shortNames"] as? [[String]]) {
    #expect(HarnessSwitchModel.shortName(pair[0]) == pair[1])
  }
  for pair in try #require(vector["readableNames"] as? [[String]]) {
    #expect(HarnessSwitchModel.readableName(ofHarnessId: pair[0]) == pair[1])
  }
  let labels = try #require(vector["actionLabels"] as? [String: String])
  var target = try decode(HarnessInfo.self, try #require(known["codex"]))
  target.name = try #require(labels["target"])
  #expect(HarnessSwitchModel.actionLabel(for: target) == labels["withTarget"])
  #expect(HarnessSwitchModel.actionLabel(for: nil) == labels["withoutTarget"])
}

@Test func harnessChipTitlesFollowTheSharedVectors() throws {
  let vector = try vector("harness-picker")
  for entry in try cases(vector, "chipTitles") {
    let current = try #require(entry["current"] as? String)
    let target = entry["switchingTo"] as? String
    let control = HarnessSwitchModel.control(
      current: current,
      switchState: target.map {
        AgentHarnessSwitchView(fromHarness: current, toHarness: $0, phase: .switching)
      },
      harnesses: try catalogue(vector, entry["harnesses"]), paneName: entry["paneName"] as? String)
    #expect(control.title == entry["title"] as? String, "\(entry["name"] ?? "")")
    #expect(control.busy == entry["busy"] as? Bool, "\(entry["name"] ?? "")")
  }
}

@Test func usageReadingsFollowTheSharedVectors() throws {
  let vector = try vector("usage-presentation")
  for entry in try cases(vector, "readings") {
    let reading = try decode(UsageReading.self, try #require(entry["reading"]))
    let shown = UsagePresentation(reading)
    let name = "\(entry["name"] ?? "")"
    #expect(shown.displayedWindow?.label == entry["window"] as? String, "\(name)")
    #expect(shown.percentage == entry["percentage"] as? String, "\(name)")
    let band: String? = shown.band.map {
      switch $0 {
      case .success: "success"
      case .warning: "warning"
      case .danger: "danger"
      }
    }
    #expect(band == entry["band"] as? String, "\(name)")
    let emphasis =
      switch shown.emphasis {
      case .current: "current"
      case .lastKnown: "lastKnown"
      case .none: "none"
      }
    #expect(emphasis == entry["emphasis"] as? String, "\(name)")
    #expect(shown.unavailableMessage == entry["unavailableMessage"] as? String, "\(name)")
    #expect(shown.staleHint == entry["staleHint"] as? String, "\(name)")
  }
  for pair in try #require(vector["durations"] as? [[Any]]) {
    let seconds = try #require(pair[0] as? Double)
    #expect(UsageTimeText.duration(seconds) == pair[1] as? String)
  }
  let resets = try #require(vector["resets"] as? [String: Any])
  let nowText = try #require(resets["now"] as? String)
  let now = try #require(UsageTimestamp.date(from: nowText))
  for entry in try cases(resets, "cases") {
    #expect(
      UsageTimeText.reset(entry["resetsAt"] as? String, now: now) == entry["text"] as? String,
      "\(entry["name"] ?? "")")
  }
  let activities = try #require(vector["activities"] as? [String: Any])
  for entry in try cases(activities, "cases") {
    let activity = try decode(UsageActivity.self, try #require(entry["activity"]))
    let name = "\(entry["name"] ?? "")"
    #expect(UsageActivityText.summary(activity) == entry["summary"] as? String, "\(name)")
    #expect(UsageActivityText.line(activity) == entry["line"] as? String, "\(name)")
  }
  let notices = try #require(activities["notices"] as? [String: Any])
  for entry in try cases(notices, "cases") {
    let reading = try decode(UsageReading.self, try #require(entry["reading"]))
    let model = UsageDetailModel(reading, onThisMac: true)
    var shown: String?
    if case .notice(let notice) = model.content, model.reset == "No plan limit" {
      shown = notice.message
    }
    #expect(shown == entry["notice"] as? String, "\(entry["name"] ?? "")")
  }
  for pair in try #require(vector["sources"] as? [[String]]) {
    let source = try decode(UsageSource.self, pair[0])
    #expect(source.displayName == pair[1])
  }
}

private func session(_ fields: [String: Any]) throws -> AgentSession {
  var base: [String: Any] = [
    "harness": "claude", "version": "1", "fingerprint": "f", "options": [], "authMethods": [],
    "queue": [], "draft": false, "role": "worker",
  ]
  base.merge(fields) { _, new in new }
  return try decode(AgentSession.self, base)
}

@Test func accountActionsFollowTheSharedVectors() throws {
  let vector = try vector("agent-account")
  let copy = try #require(vector["copy"] as? [String: String])
  #expect(AgentAccountActions.sectionTitle == copy["sectionTitle"])
  #expect(AgentAccountActions.explanation == copy["explanation"])
  for entry in try cases(vector, "account") {
    var fields: [String: Any] = [
      "reauthMethods": entry["reauthMethods"] ?? [], "canSignOut": entry["canSignOut"] ?? false,
    ]
    if entry["operation"] as? Bool == true {
      fields["operation"] = [
        "operationId": "o", "configId": "model", "value": "m", "startedAt": "2026-10-01T00:00:00Z",
      ]
    }
    let state = try decode(PaneState.self, try #require(entry["paneState"]))
    let actions = AgentAccountActions(
      agent: try session(fields), paneState: state,
      connected: entry["connected"] as? Bool ?? false)
    let name = "\(entry["name"] ?? "")"
    guard let expected = entry["expected"] as? [String: Any] else {
      #expect(actions == nil, "\(name)")
      continue
    }
    let shown = try #require(actions, "\(name)")
    #expect(shown.isEnabled == expected["enabled"] as? Bool, "\(name)")
    let items = shown.items.map { item -> [String] in
      switch item.action {
      case .switchTo(let id): ["switch:\(id)", item.title]
      case .signOut: ["signOut", item.title]
      }
    }
    #expect(items == expected["items"] as? [[String]], "\(name)")
  }
}
