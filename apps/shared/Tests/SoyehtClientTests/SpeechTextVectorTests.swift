import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/speech-text.json`, which the Windows and Linux app passes too.
private func vector() throws -> JsonValue { try ContractVector.load("speech-text") }

private func chunker(_ item: JsonValue, _ defaults: JsonValue) -> SentenceChunker {
  func length(_ key: String) -> Int? { item["config"][key].int ?? defaults[key].int }
  return SentenceChunker(
    minimumLength: length("minimumLength") ?? 0, maximumLength: length("maximumLength") ?? 0)
}

@Test func whatIsSpokenIsWhatTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "speechText") {
    let spoken = SpeechText.speakable(
      item["markdown"].string ?? "", streaming: item["streaming"].bool == true)
    #expect(spoken == item["spoken"].string, "\(item["name"].string ?? "")")
  }
}

@Test func sentencesAreCutAsTheVectorSays() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "chunker") {
    var chunker = chunker(item, all["chunkerDefaults"])
    for (index, update) in (item["updates"].array ?? []).enumerated() {
      let emitted = chunker.update(
        update["spoken"].string ?? "", final: update["final"].bool == true)
      let expected = (update["emit"].array ?? []).compactMap(\.string)
      #expect(emitted == expected, "\(item["name"].string ?? "") update \(index)")
    }
  }
}

@Test func aReplyIsSpokenAsItArrivesAsTheVectorSays() throws {
  let all = try vector()
  for item in ContractVector.cases(all, "stream") {
    var chunker = chunker(item, all["chunkerDefaults"])
    for (index, update) in (item["updates"].array ?? []).enumerated() {
      let final = update["final"].bool == true
      let spoken = SpeechText.speakable(update["markdown"].string ?? "", streaming: !final)
      let expected = (update["emit"].array ?? []).compactMap(\.string)
      #expect(
        chunker.update(spoken, final: final) == expected,
        "\(item["name"].string ?? "") update \(index)")
    }
  }
}

@Test func theHeardPartIsWhatTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "heard") {
    let heard = HeardSpeech.text(
      played: (item["played"].array ?? []).compactMap(\.string), current: item["current"].string,
      fraction: item["fraction"].double ?? 0)
    #expect(heard == item["heard"].string, "\(item["name"].string ?? "")")
  }
}
