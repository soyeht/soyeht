import AVFoundation
import Foundation
import Testing

@testable import SoyehtClient

/// A microphone that never opens: the test pushes buffers into the sink itself.
@MainActor private final class FakeCapture: AudioCapturing {
  private(set) var sink: MicrophoneCapture.Sink?
  private(set) var stops = 0
  var onEnded: (@MainActor (any Error) -> Void)?

  func start(_ sink: @escaping MicrophoneCapture.Sink) throws { self.sink = sink }
  func stop() { stops += 1 }
}

/// The microphone hears 100 ms of silence.
@MainActor private func hear(_ capture: FakeCapture) throws {
  let sink = try #require(capture.sink)
  sink(try silentBuffer())
}

@MainActor private struct Rig {
  let recognizer: CloudSpeechRecognizer
  let capture = FakeCapture()
  let transport: FakeSpeechTransport
  let vault: MemoryProviderKeyVault
  let microphoneAsked: Counter

  @MainActor final class Counter { var count = 0 }

  init(vault: MemoryProviderKeyVault = MemoryProviderKeyVault(["cloud.example": "sk-test-1"])) {
    transport = FakeSpeechTransport { _ in (200, Data(#"{"text":"open the editor"}"#.utf8)) }
    self.vault = vault
    let asked = Counter()
    microphoneAsked = asked
    let capture = capture
    recognizer = CloudSpeechRecognizer(
      requests: CloudSpeechRequests(
        configuration: cloudConfiguration(), vault: vault, transport: transport),
      capture: capture,
      microphone: {
        asked.count += 1
        return .authorized
      })
  }
}

@MainActor private func events(
  _ rig: Rig, options: SpeechSessionOptions = SpeechSessionOptions(), until: (SpeechEvent) -> Bool
) async throws -> [SpeechEvent] {
  var received: [SpeechEvent] = []
  try rig.recognizer.start(options) { received.append($0) }
  try hear(rig.capture)
  rig.recognizer.finish()
  for _ in 0..<500 where !received.contains(where: until) { await Task.yield() }
  return received
}

@MainActor @Test func theRecordingGoesToTheProviderWhenTheListenerStopsAndTheAnswerIsFinal()
  async throws
{
  let rig = Rig()
  let received = try await events(
    rig, options: SpeechSessionOptions(language: "pt-BR", vocabulary: ["Soyeht"])
  ) { $0 == .final("open the editor") }
  #expect(received == [.final("open the editor")])
  #expect(rig.capture.stops >= 1)
  let sent = try #require(rig.transport.requests.first)
  #expect(
    String(decoding: sent.httpBody ?? Data(), as: UTF8.self).contains(
      "name=\"prompt\"\r\n\r\nSoyeht"))
}

@MainActor @Test func aCaptureThatEndsWithoutBeingStoppedIsReportedAsAFailure() throws {
  struct Gone: LocalizedError { var errorDescription: String? { "The audio went away." } }
  let rig = Rig()
  var received: [SpeechEvent] = []
  try rig.recognizer.start(SpeechSessionOptions()) { received.append($0) }
  try hear(rig.capture)
  rig.capture.onEnded?(Gone())
  #expect(received == [.failed("The audio went away.")])
  #expect(rig.transport.requests.isEmpty)
}

@MainActor @Test func nothingIsSentWhileTheListenerIsStillListening() throws {
  let rig = Rig()
  try rig.recognizer.start(SpeechSessionOptions()) { _ in }
  #expect(rig.transport.requests.isEmpty)
}

@MainActor @Test func cancellingSendsNothingAndReportsNothing() async throws {
  let rig = Rig()
  var received: [SpeechEvent] = []
  try rig.recognizer.start(SpeechSessionOptions()) { received.append($0) }
  try hear(rig.capture)
  rig.recognizer.cancel()
  rig.recognizer.finish()
  for _ in 0..<50 { await Task.yield() }
  #expect(rig.transport.requests.isEmpty)
  #expect(received.isEmpty)
}

@MainActor @Test func aProviderThatRefusesTheKeyIsReportedAsAFailure() async throws {
  let rig = Rig()
  let refusing = FakeSpeechTransport { _ in (401, Data("sk-test-1".utf8)) }
  let recognizer = CloudSpeechRecognizer(
    requests: CloudSpeechRequests(
      configuration: cloudConfiguration(), vault: rig.vault, transport: refusing),
    capture: rig.capture, microphone: { .authorized })
  var received: [SpeechEvent] = []
  try recognizer.start(SpeechSessionOptions()) { received.append($0) }
  try hear(rig.capture)
  recognizer.finish()
  for _ in 0..<500 where received.isEmpty { await Task.yield() }
  #expect(received == [.failed("The provider did not accept the key.")])
}

@MainActor @Test func withoutAKeyTheListenerAsksNeitherTheMicrophoneNorTheProvider() async throws {
  let rig = Rig(vault: MemoryProviderKeyVault())
  #expect(
    await rig.recognizer.authorize()
      == .unavailable("Add the provider's API key first."))
  #expect(rig.microphoneAsked.count == 0)
  #expect(throws: CloudSpeechError.self) {
    try rig.recognizer.start(SpeechSessionOptions()) { _ in }
  }
  #expect(rig.capture.sink == nil)
  #expect(rig.transport.requests.isEmpty)
}

@MainActor @Test func withAKeyTheMicrophonePermissionIsTheLastStep() async {
  let rig = Rig()
  #expect(await rig.recognizer.authorize() == .authorized)
  #expect(rig.microphoneAsked.count == 1)
}

@Test func aRecordingStopsGrowingAtItsLimit() {
  let recording = CloudSpeechRecognizer.Recording(limit: 5)
  recording.append([1, 2, 3])
  recording.append([4, 5, 6, 7])
  #expect(recording.samples == [1, 2, 3, 4, 5])
}

@MainActor @Test func aRecordingWithNothingInItIsNotSentAndTheTranscriptIsEmpty() async throws {
  let rig = Rig()
  var received: [SpeechEvent] = []
  try rig.recognizer.start(SpeechSessionOptions()) { received.append($0) }
  rig.recognizer.finish()
  for _ in 0..<200 where received.isEmpty { await Task.yield() }
  #expect(received == [.final("")])
  #expect(rig.transport.requests.isEmpty)
}

@MainActor @Test func theCallerIsToldWhenTheTenMinuteLimitCutTheRecording() async throws {
  let rig = Rig()
  var received: [SpeechEvent] = []
  try rig.recognizer.start(SpeechSessionOptions()) { received.append($0) }
  #expect(!rig.recognizer.recordingWasCut)
  rig.recognizer.finish()
  #expect(!rig.recognizer.recordingWasCut)

  // The limit is a number of samples, so the cut is proven on the recording itself.
  let recording = CloudSpeechRecognizer.Recording(limit: 3)
  recording.append([1, 2])
  #expect(!recording.wasCut)
  recording.append([3, 4])
  #expect(recording.wasCut)
  #expect(recording.samples == [1, 2, 3])
  #expect(CloudSpeechRecognizer.maximumSeconds == 600)
}
