import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The generated rules of `contracts/protocol.json` (`x-settings`, `x-clientPolicy`) and the
/// vectors that fix what they come to, which the desktop client passes as well.

/// `contracts/protocol.json`, which the generated rules come from.
private func contract() throws -> JsonValue {
  try JSONDecoder().decode(
    JsonValue.self,
    from: Data(
      contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().appendingPathComponent("contracts/protocol.json")))
}

@Test func everyPaneStateIsPresentedAsTheVectorSays() throws {
  let vector = try ContractVector.load("pane-state-presentation")
  let cases = ContractVector.cases(vector, "states")
  guard case .object(let states) = try contract()["x-clientPolicy"]["paneStates"] else {
    Issue.record("x-clientPolicy.paneStates is missing")
    return
  }
  #expect(Set(cases.compactMap { $0["state"].string }) == Set(states.keys))
  for item in cases {
    let state = try #require(item["state"].string.flatMap(PaneState.init(rawValue:)))
    #expect(state.dot.rawValue == item["dot"].string, "\(state)")
    #expect(state.isTurnRunning == (item["turnRunning"].bool == true), "\(state)")
    #expect(
      AgentPaneStatusPresentation.title(state: state, needsAuthentication: false, hasSession: false)
        == item["word"].string, "\(state)")
  }
}

@MainActor @Test func everyDotRoleIsOnePaletteColor() {
  #expect(StateDot.success.color == SoyehtThemeColors.success)
  #expect(StateDot.accent.color == SoyehtThemeColors.accent)
  #expect(StateDot.warning.color == SoyehtThemeColors.warning)
  #expect(StateDot.muted.color == SoyehtThemeColors.muted)
  #expect(StateDot.secondary.color == SoyehtThemeColors.secondary)
  #expect(StateDot.allCases.count == 5)
}

@Test func theStatusWordFollowsTheVector() throws {
  let vector = try ContractVector.load("pane-state-presentation")
  for item in ContractVector.cases(vector, "statusWords") {
    let state = try #require(item["state"].string.flatMap(PaneState.init(rawValue:)))
    #expect(
      AgentPaneStatusPresentation.title(
        state: state, needsAuthentication: item["needsAuthentication"].bool == true,
        hasSession: item["hasSession"].bool == true) == item["title"].string,
      "\(item["name"].string ?? "")")
  }
}

@Test func theReconnectWaitIsTheOneInTheVector() throws {
  let vector = try ContractVector.load("reconnect")
  let expected = (vector["waitsMs"].array ?? []).compactMap(\.int).map { Duration.milliseconds($0) }
  var backoff = ReconnectBackoff()
  #expect((0..<expected.count).map { _ in backoff.next() } == expected)
  backoff.reset()
  #expect(backoff.next() == .milliseconds(try #require(vector["afterResetMs"].int)))
}

@Test func theGeneratedSettingsKnowTheirKeysDefaultsAndBounds() throws {
  guard case .object(let settings) = try contract()["x-settings"] else {
    Issue.record("x-settings is missing")
    return
  }
  let alone = settings.keys.filter { !$0.contains("<") }
  #expect(Set(alone) == Set(Setting.keys))
  for (key, rule) in settings {
    let concrete = key.replacingOccurrences(of: "<harnessId>", with: "claude")
    let value = rule["default"]
    #expect(Setting.defaultValue(for: concrete) == value, "\(key)")
    #expect(Setting.accepts(value, for: concrete), "\(key)")
  }
  #expect(Setting.defaultValue(for: "web.openLocalLinksInSoyeht") == nil)
  #expect(!Setting.accepts(.bool(true), for: "web.openLocalLinksInSoyeht"))
  #expect(Setting.accepts(.integer(1000), for: Setting.terminalDefaultCols))
  #expect(!Setting.accepts(.integer(0), for: Setting.terminalDefaultCols))
  #expect(!Setting.accepts(.integer(1001), for: Setting.terminalDefaultRows))
  #expect(!Setting.accepts(.string("80"), for: Setting.terminalDefaultRows))
  #expect(!Setting.accepts(.string("true"), for: Setting.recordDefault))
  #expect(
    Setting.accepts(.string(String(repeating: "x", count: 128)), for: Setting.harnessPreferredId))
  #expect(
    !Setting.accepts(.string(String(repeating: "x", count: 129)), for: Setting.harnessPreferredId))
  #expect(!Setting.accepts(.string("a\0b"), for: Setting.harnessPreferredId))
}

@Test func aHarnessDefaultModeKeyNamesItsHarnessWithinTheBounds() {
  let key = Setting.harnessDefaultModeKey("codex")
  #expect(Setting.harnessDefaultModeParameter(of: key) == "codex")
  #expect(Setting.harnessDefaultModeParameter(of: Setting.harnessPreferredId) == nil)
  #expect(Setting.harnessDefaultModeParameter(of: Setting.harnessDefaultModePrefix) == nil)
  let longest = String(repeating: "h", count: 128)
  #expect(
    Setting.harnessDefaultModeParameter(of: Setting.harnessDefaultModeKey(longest)) == longest)
  #expect(
    Setting.harnessDefaultModeParameter(of: Setting.harnessDefaultModeKey(longest + "h")) == nil)
}

@Test func theInlineImageTypesAreThoseTheAgentsTakeInline() {
  #expect(ClientPolicy.inlineImageTypes == ["image/png", "image/jpeg", "image/gif", "image/webp"])
  #expect(AgentAttachmentStaging.inlineImageTypes == [.png, .jpeg, .gif, .webP])
}

@Test func permissionPreviewsAreTheCasesTheDesktopPasses() throws {
  let vector = try ContractVector.load("permission-preview")
  #expect(PermissionPreviewPolicy.visibleLineLimit == vector["visibleLineLimit"].int)
  for item in ContractVector.cases(vector, "previews") {
    let newText =
      item["newLineCount"].int.map { String(repeating: "line\n", count: $0) }
      ?? item["newText"].string ?? ""
    let preview = PermissionPreviewPolicy.preview(
      PermissionFileChange(path: "a.rs", oldText: item["oldText"].string, newText: newText))
    let name = item["name"].string ?? ""
    #expect(preview.unavailable == (item["unavailable"].bool == true), "\(name)")
    if let expected = item["rows"].array {
      let shown = preview.rows.map { ($0.kind == .removed ? "-" : "+") + $0.text }
      let wanted = expected.map { ($0.array?[0].string ?? "") + ($0.array?[1].string ?? "") }
      #expect(shown == wanted, "\(name)")
    }
  }
}

@Test func linesAreCountedAsTheVectorSays() throws {
  let vector = try ContractVector.load("permission-preview")
  for item in ContractVector.cases(vector, "lineCounts") {
    #expect(
      PermissionPreviewPolicy.lineCount(item["text"].string ?? "") == item["lines"].int,
      "\(item["name"].string ?? "")")
  }
}
