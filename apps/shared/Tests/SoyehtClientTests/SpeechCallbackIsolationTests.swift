@preconcurrency import AVFoundation
import Foundation
import Speech
import Testing

@testable import SoyehtClient

// The system answers speech and microphone requests on its own queues. Each test calls the
// handler from a global queue while the awaiting side is on the main actor; a handler that
// inherited main-actor isolation would trap there instead of returning.

@MainActor @Test func aSystemHandlerCalledFromABackgroundQueueResumesTheCaller() async {
  let status: SFSpeechRecognizerAuthorizationStatus = await SystemCallback.value { handler in
    DispatchQueue.global().async { handler(.authorized) }
  }
  #expect(status == .authorized)

  let granted: Bool = await SystemCallback.value { handler in
    DispatchQueue.global(qos: .utility).async { handler(false) }
  }
  #expect(!granted)
}

@MainActor @Test func aRecognitionResultFromABackgroundQueueReachesTheMainActor() async {
  let event: SpeechEvent = await withCheckedContinuation { continuation in
    let handler = AppleSpeechRecognizer.report { continuation.resume(returning: $0) }
    DispatchQueue.global().async { handler(nil, SpeechTestError.refused) }
  }
  #expect(event == .failed(SpeechTestError.refused.localizedDescription))
}

@MainActor @Test func theAudioTapRunsOnTheAudioThreadWithoutAnActor() async throws {
  let request = SFSpeechAudioBufferRecognitionRequest()
  let format = try #require(AVAudioFormat(standardFormatWithSampleRate: 16_000, channels: 1))
  let buffer = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 160))
  buffer.frameLength = 160
  let tap = AppleSpeechRecognizer.feed(request)
  await withCheckedContinuation { continuation in
    DispatchQueue.global(qos: .userInteractive).async {
      tap(buffer)
      continuation.resume()
    }
  }
}

@Test func aRecognizerCallbackBecomesOneEvent() {
  #expect(SpeechEvent(text: "go", isFinal: true, failure: nil) == .final("go"))
  #expect(SpeechEvent(text: "go", isFinal: false, failure: "x") == .partial("go"))
  #expect(SpeechEvent(text: nil, isFinal: false, failure: "x") == .failed("x"))
  #expect(SpeechEvent(text: nil, isFinal: true, failure: nil) == nil)
}

private enum SpeechTestError: LocalizedError {
  case refused
  var errorDescription: String? { "refused" }
}
