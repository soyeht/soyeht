import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The editor cases of `contracts/vectors/editor.json`, which the desktop client passes as well.

private func strings(_ value: JsonValue) -> [String] {
  (value.array ?? []).compactMap(\.string)
}

private func name(_ item: JsonValue) -> String { item["name"].string ?? "" }

@Test func theViewportPageIsTheProtocolLimit() throws {
  let vector = try ContractVector.load("editor")
  #expect(Int(EditorViewportPaging.pageLines) == vector["pageLines"].int)
}

@Test func theVisibleLinesAskForAnotherPageAsTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "requestedStart") {
    func line(_ key: String) -> UInt32 { UInt32(item[key].int ?? 0) }
    let start = EditorViewportPaging.requestedStart(
      visibleFrom: line("visibleFrom"), visibleLines: line("visibleLines"),
      loadedFrom: line("loadedFrom"), loadedTo: line("loadedTo"), totalLines: line("totalLines"),
      askedLines: item["askedLines"].int.map { UInt32($0) })
    #expect(start.map(Int.init) == item["start"].int, "\(name(item))")
  }
}

@Test func aLineOutsideThePageLandsAsTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "landingStart") {
    func line(_ key: String) -> UInt32 { UInt32(item[key].int ?? 0) }
    let start = EditorViewportPaging.landingStart(
      line: line("line"), loadedFrom: line("loadedFrom"), loadedTo: line("loadedTo"),
      askedLines: item["askedLines"].int.map { UInt32($0) })
    #expect(Int(start) == item["start"].int, "\(name(item))")
  }
}

@Test func theTabRulesAreTheOnesInTheVector() throws {
  let tabs = try ContractVector.load("editor")["tabs"]
  for item in ContractVector.cases(tabs, "active") {
    let active = EditorTabPolicy.active(
      selected: item["selected"].string, tabs: strings(item["tabs"]),
      history: strings(item["history"]))
    #expect(active == item["tab"].string, "\(name(item))")
  }
  for item in ContractVector.cases(tabs, "successor") {
    let next = EditorTabPolicy.successor(
      closing: item["closing"].string ?? "", tabs: strings(item["tabs"]),
      history: strings(item["history"]))
    #expect(next == item["tab"].string, "\(name(item))")
  }
  for item in ContractVector.cases(tabs, "cycled") {
    let next = EditorTabPolicy.cycled(
      from: item["current"].string, tabs: strings(item["tabs"]),
      forward: item["forward"].bool == true)
    #expect(next == item["tab"].string, "\(name(item))")
  }
  for item in ContractVector.cases(tabs, "recorded") {
    let history = EditorTabPolicy.recorded(
      item["pane"].string ?? "", in: strings(item["history"]))
    #expect(history == strings(item["historyAfter"]), "\(name(item))")
  }
  let cap = tabs["historyCap"]
  let many = (0..<(cap["historyCount"].int ?? 0)).map { "p\($0)" }
  let capped = EditorTabPolicy.recorded(cap["pane"].string ?? "", in: many)
  #expect(capped.count == cap["length"].int)
  #expect(capped.first == cap["pane"].string)
}

@Test func aPreviewStaysOneUntilTheFileChangesAsTheVectorSays() throws {
  let tabs = try ContractVector.load("editor")["tabs"]
  for item in ContractVector.cases(tabs, "preview") {
    let tab = EditorTab(
      paneId: "p", fileId: "f", path: "a.rs", preview: item["preview"].bool == true,
      previewRevision: item["previewRevision"].string)
    let file = item["fileRevision"].string.map {
      FileDocument(
        fileId: "f", paneId: "p", workspaceId: "w", path: "a.rs", language: .rust, revision: $0,
        diskRevision: $0, lineCount: 1, byteCount: "1", dirty: false, conflicted: false,
        watching: false)
    }
    #expect(
      EditorTabPolicy.isPreview(tab, file: file) == (item["isPreview"].bool == true),
      "\(name(item))")
  }
}

private func ranges(_ value: JsonValue) -> [NSRange] {
  (value.array ?? []).map {
    NSRange(location: $0.array?[0].int ?? 0, length: $0.array?[1].int ?? 0)
  }
}

private func position(_ value: JsonValue) -> TextPosition {
  TextPosition(
    line: UInt32(value.array?[0].int ?? 0), column: UInt32(value.array?[1].int ?? 0))
}

@Test func keystrokesChangeAndSendWhatTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "editing") {
    let text = item["text"].string ?? ""
    let start = UInt32(item["viewportStartLine"].int ?? 0)
    let selected = ranges(item["ranges"])
    let plan: EditorInputPlan?
    switch item["kind"].string {
    case "insert":
      plan = EditorEditing.insertionPlan(
        text: text, viewportStartLine: start, ranges: selected,
        inserted: item["inserted"].string ?? "", language: .rust)
    case "delete":
      plan = EditorEditing.deletionPlan(
        text: text, viewportStartLine: start, ranges: selected,
        backward: item["backward"].bool == true, language: .rust)
    default:
      plan = EditorEditing.verbatimPlan(
        text: text, viewportStartLine: start, ranges: selected,
        strings: strings(item["strings"]))
    }
    let made = try #require(plan, "\(name(item))")
    #expect(
      EditorEditing.applying(made.replacements, to: text) == item["result"].string,
      "\(name(item))")
    #expect(made.selections == ranges(item["selections"]), "\(name(item))")
    let edits = (item["edits"].array ?? []).map {
      TextEdit(
        range: SoyehtProtocol.TextRange(start: position($0["start"]), end: position($0["end"])),
        text: $0["text"].string ?? "")
    }
    #expect(made.edits.count == edits.count, "\(name(item))")
    for (made, wanted) in zip(made.edits, edits) {
      #expect(made.range.start.line == wanted.range.start.line, "\(name(item))")
      #expect(made.range.start.column == wanted.range.start.column, "\(name(item))")
      #expect(made.range.end.line == wanted.range.end.line, "\(name(item))")
      #expect(made.range.end.column == wanted.range.end.column, "\(name(item))")
      #expect(made.text == wanted.text, "\(name(item))")
    }
  }
}

@Test func theProjectTreeAnswersTheKeysAsTheVectorSays() throws {
  let vector = try ContractVector.load("editor")["projectTree"]
  var tree = EditorProjectTree()
  func list(_ directory: String) {
    let entries = (vector["listings"][directory].array ?? []).map { entry in
      ProjectEntry(
        name: entry.array?[0].string ?? "",
        path: EditorProjectTree.join(
          directory, entry.array?[0].string ?? ""),
        kind: entry.array?[1].bool == true ? .directory : .file, ignored: false)
    }
    tree.apply(
      ProjectTreeListResult(
        workspaceId: "w", path: directory, entries: entries, truncated: false))
  }
  list("")
  for directory in strings(vector["expanded"]) {
    tree.expand(directory)
    list(directory)
  }
  #expect(tree.rows.map(\.node.path) == strings(vector["rows"]))
  for step in ContractVector.cases(vector, "steps") {
    let key: EditorProjectTree.Key
    switch step["key"].string {
    case "up": key = .up
    case "down": key = .down
    case "left": key = .left
    case "right": key = .right
    case "first": key = .first
    default: key = .last
    }
    let loads = tree.handle(key)
    #expect(tree.selection == step["selection"].string, "\(step["key"].string ?? "")")
    #expect(loads == step["loads"].string, "\(step["key"].string ?? "")")
    if step["rows"].array != nil {
      #expect(tree.rows.map(\.node.path) == strings(step["rows"]))
    }
  }
}

@Test func theLineBreakOfAFileIsTheOneTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "lineEndings") {
    let lines = strings(item["lines"])
    #expect(
      EditorEditing.lineEnding(lines: lines, line: item["line"].int ?? 0) == item["ending"].string,
      "\(name(item))")
  }
}

@Test func readsSettleAfterAPageTheEngineCutsShortAsTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "settling") {
    func number(_ key: String) -> UInt32 { UInt32(item[key].int ?? 0) }
    let total = number("totalLines")
    let cut = number("cutLines")
    let lines = number("visibleLines")
    var page: (from: UInt32, to: UInt32, asked: UInt32) = (0, min(200, total), 200)
    func ask(_ from: UInt32) -> UInt32? {
      EditorViewportPaging.requestedStart(
        visibleFrom: from, visibleLines: lines, loadedFrom: page.from, loadedTo: page.to,
        totalLines: total, askedLines: page.asked)
    }
    var reads: [Int] = []
    while let start = ask(number("visibleFrom")), reads.count < 8 {
      reads.append(Int(start))
      page = (
        start, start + min(EditorViewportPaging.pageLines, cut, total - start),
        EditorViewportPaging.pageLines
      )
    }
    #expect(reads == (item["reads"].array ?? []).compactMap(\.int), "\(name(item))")
    #expect(
      [Int(page.from), Int(page.to)] == (item["page"].array ?? []).compactMap(\.int),
      "\(name(item))")
    #expect(ask(number("visibleFrom") + 1) == nil, "\(name(item))")
  }
}

@Test func theCaretReachesALinePastAPageTheEngineCutShortAsTheVectorSays() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector, "navigation") {
    func number(_ key: String) -> UInt32 { UInt32(item[key].int ?? 0) }
    let total = number("totalLines")
    let target = number("targetLine")
    let cuts = (item["cuts"].array ?? []).compactMap(\.int).map { UInt32($0) }
    var page = (from: number("loadedFrom"), to: number("loadedTo"))
    var reads: [Int] = []
    while !(page.from <= target && target < page.to), reads.count < 5 {
      // The first read lands on the line; one that misses it is read again from it.
      let start =
        reads.isEmpty
        ? EditorViewportPaging.landingStart(
          line: target, loadedFrom: page.from, loadedTo: page.to,
          askedLines: EditorViewportPaging.pageLines)
        : target
      let cut = cuts[min(reads.count, cuts.count - 1)]
      reads.append(Int(start))
      page = (start, start + min(EditorViewportPaging.pageLines, cut, total - start))
    }
    #expect(reads == (item["reads"].array ?? []).compactMap(\.int), "\(name(item))")
  }
}
