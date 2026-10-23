import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/presentation.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("presentation") }

private func number(_ value: JsonValue) throws -> Double { try #require(value.double) }

private func input(_ name: String?) throws -> WritingSlideShowInput {
  switch name {
  case "rightArrow": .rightArrow
  case "leftArrow": .leftArrow
  case "pageDown": .pageDown
  case "pageUp": .pageUp
  case "space": .space
  case "click": .click
  case "escape": .escape
  default: .other
  }
}

private func command(_ name: String?) -> WritingSlideShowCommand? {
  switch name {
  case "advance": .advance
  case "previous": .previous
  case "exit": .exit
  default: nil
  }
}

@Test func aSlideIsNamedAsTheVectorSays() throws {
  let vector = try vector()
  for item in ContractVector.cases(vector, "slideTitles") {
    let heading = try #require(item["heading"].string)
    #expect(WritingDisplayText.slideTitle(heading) == item["title"].string)
  }
  for item in ContractVector.cases(vector, "slideNotes") {
    let notes = try #require(item["notes"].string)
    #expect(WritingDisplayText.slideNotes(notes) == item["line"].string)
  }
  #expect(WritingDisplayText.presentationHint == vector["hint"]["line"].string)
  #expect(WritingDisplayText.presentationFormat == vector["hint"]["format"].string)
}

@Test func aLineBelongsToTheSlideTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "slideAtLine") {
    let slides = try (item["slides"].array ?? []).enumerated().map { at, range in
      let ends = try #require(range.array)
      return WritingSlide(
        index: UInt32(at), startLine: UInt32(try #require(ends[0].int)),
        endLine: UInt32(try #require(ends[1].int)), heading: "", notes: "", imagePaths: [])
    }
    let line = UInt32(try #require(item["line"].int))
    let index = WritingSlideSelection.index(containing: line, in: slides)
    #expect(index.map(Int.init) == item["index"].int, "\(item["name"].string ?? "")")
  }
}

@Test func theSlideShowAnswersEachInputAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "showCommands") {
    let got = WritingSlideShowPolicy.standard.command(
      for: try input(item["input"].string), commandModified: item["modified"].bool ?? false)
    #expect(got == command(item["command"].string), "\(item["input"].string ?? "")")
  }
}

@Test func theSlideShowMovesAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "showStates") {
    let name = item["name"].string ?? ""
    var state = WritingSlideShowState(
      index: try #require(item["index"].int), slideCount: try #require(item["count"].int))
    for step in item["steps"].array ?? [] {
      var exits = false
      if let count = step["reconcile"].int {
        exits = state.reconcile(slideCount: count)
      } else if let applied = command(step["apply"].string) {
        exits = state.apply(applied)
      }
      #expect(state.index == step["index"].int, "\(name)")
      #expect(exits == step["exit"].bool, "\(name)")
    }
  }
}

@Test func theSlideShowStageAndSlideSizesAreTheVectorsOwn() throws {
  let vector = try vector()
  for item in ContractVector.cases(vector, "showStage") {
    let viewport = try #require(item["viewport"].array)
    let stage = WritingSlideShowPolicy.standard.fittedStage(
      in: PaneLayoutSize(
        width: try number(viewport[0]), height: try number(viewport[1])))
    let want = item["stage"]
    #expect(stage.origin.x == (try number(want["x"])))
    #expect(stage.origin.y == (try number(want["y"])))
    #expect(stage.size.width == (try number(want["width"])))
    #expect(stage.size.height == (try number(want["height"])))
  }
  for item in ContractVector.cases(vector, "slideMetrics") {
    let size = try #require(item["stage"].array)
    let metrics = WritingSlideShowPolicy.standard.slideSizing.metrics(
      in: PaneLayoutSize(width: try number(size[0]), height: try number(size[1])))
    #expect(metrics.titleSize == (try number(item["titleSize"])))
    #expect(metrics.contentSpacing == (try number(item["spacing"])))
    #expect(metrics.padding == (try number(item["padding"])))
  }
}
