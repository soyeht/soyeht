import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let word: [InkStroke] = [
  [InkPoint(x: 1.5, y: 2, t: 0), InkPoint(x: 3, y: 9, t: 16)],
  [InkPoint(x: 0, y: 5, t: 120), InkPoint(x: 4, y: 5, t: 150)],
]

@MainActor @Test func theStrokesGoToTheEngineAsDrawnAndTheCandidatesComeBack() async throws {
  let client = EngineClient(socket: "/tmp/sn-ink/engine.sock")
  client.requestStub = { method, params in
    #expect(method == .inkRecognize)
    let asked = try params.decoded(InkRecognizeParams.self)
    #expect(asked.languages == ["pt-BR", "en-US"])
    #expect(asked.maxCandidates == 3)
    #expect(asked.strokes.map(\.count) == [2, 2])
    #expect(asked.strokes[0][1].x == 3)
    #expect(asked.strokes[0][1].y == 9)
    #expect(asked.strokes[0][1].t == 16)
    return try .encoded(
      InkRecognizeResult(
        candidates: [InkCandidate(text: "ola", score: 0.5), InkCandidate(text: "ala")],
        recognizer: "apple-vision", language: "pt-BR"))
  }

  let result = try await client.recognizeInk(
    strokes: word, languages: ["pt-BR", "en-US"], maxCandidates: 3)

  #expect(result.candidates.map(\.text) == ["ola", "ala"])
  #expect(result.candidates.first?.score == 0.5)
  #expect(result.candidates.last?.score == nil)
  #expect(result.language == "pt-BR")
}

@MainActor @Test func aRefusalOfTheEngineReachesTheCaller() async {
  let client = EngineClient(socket: "/tmp/sn-ink/engine.sock")
  client.requestStub = { _, _ in
    throw ClientFailure.remote(ErrorKind.ink_unsupported.rawValue, "No recognizer")
  }

  await #expect(throws: ClientFailure.self) {
    try await client.recognizeInk(strokes: word, languages: ["en"])
  }
}
