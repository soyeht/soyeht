import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/speech-text.json`, which the Windows and Linux app passes too.
/// Every key a case needs is required: a missing or misspelled one fails the test.
private func vector() throws -> JsonValue { try ContractVector.load("speech-text") }

private func strings(_ value: JsonValue, _ what: String) throws -> [String] {
  try need(value.array, what).map { try need($0.string, what) }
}

/// The chunker of a case: the policy's limits, or the ones the case overrides.
private func chunker(_ item: JsonValue) throws -> SentenceChunker {
  var minimum = ClientPolicy.voiceSentenceMinChars
  var maximum = ClientPolicy.voiceSentenceMaxChars
  if case .object(let fields) = item["config"] {
    for (key, value) in fields {
      switch key {
      case "minimumLength": minimum = try need(value.int, "config.minimumLength")
      case "maximumLength": maximum = try need(value.int, "config.maximumLength")
      default: throw ContractVectorError(description: "the vector sets \(key), which is unknown")
      }
    }
  }
  return SentenceChunker(minimumLength: minimum, maximumLength: maximum)
}

@Test func theSpeechVectorSaysWhereItsNumbersCome() throws {
  let all = try vector()
  #expect(all["policy"].string == "x-clientPolicy.voice")
  #expect(all["chunkerDefaults"] == .null)
  #expect(SentenceChunker().minimumLength == ClientPolicy.voiceSentenceMinChars)
  #expect(SentenceChunker().maximumLength == ClientPolicy.voiceSentenceMaxChars)
}

@Test func whatIsSpokenIsWhatTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "speechText") {
    let name = try need(item["name"].string, "name")
    let spoken = SpeechText.speakable(
      try need(item["markdown"].string, "\(name) markdown"),
      streaming: item["streaming"].bool == true)
    #expect(spoken == (try need(item["spoken"].string, "\(name) spoken")), "\(name)")
  }
}

@Test func sentencesAreCutAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "chunker") {
    let name = try need(item["name"].string, "name")
    var chunker = try chunker(item)
    for (index, update) in try need(item["updates"].array, "\(name) updates").enumerated() {
      let emitted = chunker.update(
        try need(update["spoken"].string, "\(name) spoken"),
        final: try need(update["final"].bool, "\(name) final"))
      #expect(emitted == (try strings(update["emit"], "\(name) emit")), "\(name) update \(index)")
    }
  }
}

@Test func aReplyIsSpokenAsItArrivesAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "stream") {
    let name = try need(item["name"].string, "name")
    var chunker = SentenceChunker()
    for (index, update) in try need(item["updates"].array, "\(name) updates").enumerated() {
      let final = try need(update["final"].bool, "\(name) final")
      let spoken = SpeechText.speakable(
        try need(update["markdown"].string, "\(name) markdown"), streaming: !final)
      #expect(
        chunker.update(spoken, final: final) == (try strings(update["emit"], "\(name) emit")),
        "\(name) update \(index)")
    }
  }
}

@Test func repliesAreFedAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "replies") {
    let name = try need(item["name"].string, "name")
    var feed = ReplyFeed()
    for (index, step) in try need(item["steps"].array, "\(name) steps").enumerated() {
      let label = "\(name) step \(index)"
      if step["cancel"].bool == true {
        feed.cancel()
      } else {
        let update = step["update"]
        let result = feed.update(
          id: try need(update["id"].string, "\(label) id"),
          markdown: try need(update["markdown"].string, "\(label) markdown"),
          final: try need(update["final"].bool, "\(label) final"))
        #expect(result.sentences == (try strings(step["emit"], "\(label) emit")), "\(label)")
        #expect(
          result.accepted == (try need(step["accepted"].bool, "\(label) accepted")), "\(label)")
      }
      #expect(feed.hasPending == (try need(step["pending"].bool, "\(label) pending")), "\(label)")
    }
  }
}

@Test func theHeardPartIsWhatTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "heard") {
    let name = try need(item["name"].string, "name")
    var timings: [SpeechTiming]?
    if case .array(let words) = item["timings"] {
      timings = try words.map {
        SpeechTiming(
          kind: .word, text: try need($0["text"].string, "\(name) timing text"),
          start: try need($0["start"].double, "\(name) timing start"))
      }
    }
    let heard = HeardSpeech.text(
      played: try strings(item["played"], "\(name) played"), current: item["current"].string,
      fraction: try need(item["fraction"].double, "\(name) fraction"), timings: timings,
      position: item["position"].double ?? 0)
    #expect(heard == (try need(item["heard"].string, "\(name) heard")), "\(name)")
  }
}

/// JSON has no NaN, so the vector cannot say it: a share that is not a number is none of the sentence.
@Test func aShareThatIsNotANumberIsNone() {
  let heard = HeardSpeech.text(played: ["A."], current: "a b", fraction: .nan)
  #expect(heard == "A.")
}
