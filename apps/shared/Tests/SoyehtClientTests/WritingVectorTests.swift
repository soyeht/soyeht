import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/writing.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("writing") }

private func section(_ name: String?) throws -> WritingSection {
  try #require(name.flatMap(WritingSection.init(rawValue:)))
}

@Test func theSectionsAreNamedAsTheVectorSays() throws {
  let cases = ContractVector.cases(try vector(), "sections")
  #expect(cases.count == WritingSection.allCases.count)
  for item in cases {
    let value = try section(item["section"].string)
    #expect(value.writingKind.rawValue == item["kind"].string)
    #expect(value.title == item["title"].string)
    #expect(value.singularTitle == item["singular"].string)
    #expect(value.subtitle == item["subtitle"].string)
    #expect(value.folderLabel == item["folder"].string)
  }
}

@Test func aSectionSpeaksAsTheVectorSays() throws {
  let vector = try vector()
  for item in ContractVector.cases(vector, "sectionCopy") {
    let value = try section(item["section"].string)
    let copy = WritingDisplayText.emptySection(value, searching: item["searching"].bool ?? false)
    #expect(copy.heading == item["emptyHeading"].string, "\(item["name"].string ?? "")")
    #expect(copy.detail == item["emptyDetail"].string, "\(item["name"].string ?? "")")
  }
  for item in ContractVector.cases(vector, "newDocumentForm") {
    let value = try section(item["section"].string)
    #expect(WritingDisplayText.newDocumentTitle(value) == item["title"].string)
    #expect(WritingDisplayText.newDocumentNote(value) == item["note"].string)
  }
}

@Test func countsReadAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "counts") {
    let kind: WritingCountKind =
      switch item["kind"].string {
      case "word": .word
      case "scene": .scene
      default: .slide
      }
    let count = try #require(item["count"].int)
    #expect(WritingDisplayText.count(count, kind: kind) == item["label"].string)
  }
}

@Test func theNewDocumentFormSubmitsAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "documentDrafts") {
    let draft = WritingDocumentDraft(
      title: try #require(item["title"].string), folder: try #require(item["folder"].string))
    let submission = draft.submission(creating: item["creating"].bool ?? false)
    let expected = item["submission"]
    let name = item["name"].string ?? ""
    if case .null = expected {
      #expect(submission == nil, "\(name)")
    } else {
      #expect(submission?.title == expected["title"].string, "\(name)")
      #expect(submission?.relativeFolder == expected["relativeFolder"].string, "\(name)")
    }
  }
}

@Test func theSearchKeepsTheEntriesTheVectorSays() throws {
  let search = try vector()["sectionFilter"]
  let entries = try (search["entries"].array ?? []).map {
    try JsonValue.object([
      "path": $0["path"], "title": $0["title"], "kind": .string("text"),
      "section": .string("texts"), "modifiedAt": .string(""), "byteCount": .string("0"),
    ]).decoded(WritingSectionEntry.self)
  }
  #expect(entries.count == 3)
  for item in search["cases"].array ?? [] {
    let kept = WritingSectionSearch.filter(entries, query: item["query"].string ?? "").map(\.path)
    let expected = (item["paths"].array ?? []).compactMap(\.string)
    #expect(kept == expected, "\(item["name"].string ?? "")")
  }
}

@Test func aPaneNamesItsFileAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "fileNames") {
    let name = WritingDisplayText.fileName(
      path: item["path"].string, fallback: try #require(item["fallback"].string))
    #expect(name == item["fileName"].string, "\(item["name"].string ?? "")")
  }
}

@Test func closingAsksAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "closing") {
    let disposition = WritingChromePolicy().closeDisposition(
      fileName: try #require(item["fileName"].string), dirty: item["dirty"].bool ?? false)
    let confirm = item["confirm"]
    let name = item["name"].string ?? ""
    if case .null = confirm {
      #expect(disposition == .closeImmediately, "\(name)")
    } else {
      let title = try #require(confirm["title"].string)
      let message = try #require(confirm["message"].string)
      #expect(disposition == .confirm(title: title, message: message), "\(name)")
    }
  }
}

@Test func formatShortcutsAreTheOnesTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "formatShortcuts") {
    let format = WritingChromePolicy.formatShortcut(
      key: try #require(item["key"].string), command: item["command"].bool ?? false)
    #expect(format?.rawValue == item["format"].string, "\(item["name"].string ?? "")")
  }
}

@Test func theSelectionToolbarOffersWhatTheVectorSays() throws {
  let toolbar = try vector()["selectionToolbar"]
  let policy = WritingChromePolicy()
  let offered = policy.plan(kind: .text, hasSelection: true).selectionActions
  let expected = toolbar["text"].array ?? []
  #expect(offered.map(\.rawValue) == expected.compactMap { $0["action"].string })
  for (action, item) in zip(offered, expected) {
    var formats = action.formatChoices.map(\.kind.rawValue)
    if let direct = action.directFormat { formats = [direct.rawValue] }
    if let destination = action.destinationFormat { formats = [destination.rawValue] }
    #expect(formats == (item["formats"].array ?? []).compactMap(\.string), "\(action)")
    #expect(
      (action.destinationFormat != nil) == (item["asksDestination"].bool ?? false), "\(action)")
  }
  #expect(
    policy.plan(kind: .text, hasSelection: false).selectionActions.map(\.rawValue)
      == (toolbar["withoutSelection"].array ?? []).compactMap(\.string))
}
