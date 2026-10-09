import Foundation
import Testing

@testable import SoyehtClient

@MainActor private final class FakeRecognizer: SpeechRecognizing {
  var access = DictationAuthorization.authorized
  var authorizations = 0
  var finished = 0
  var cancelled = 0
  var startError: Error?
  private(set) var startedWith: SpeechSessionOptions?
  private(set) var emit: (@MainActor (SpeechEvent) -> Void)?

  func authorize() async -> DictationAuthorization {
    authorizations += 1
    return access
  }

  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    if let startError { throw startError }
    startedWith = options
    emit = onEvent
  }

  func finish() { finished += 1 }
  func cancel() { cancelled += 1 }
}

@MainActor private final class ManualTimers {
  final class Timer: DictationTimer {
    let delay: Duration
    let fire: @MainActor () -> Void
    var cancelled = false
    init(delay: Duration, fire: @escaping @MainActor () -> Void) {
      self.delay = delay
      self.fire = fire
    }
    func cancel() { cancelled = true }
  }

  var timers: [Timer] = []
  var pending: Timer? { timers.last { !$0.cancelled } }

  var scheduler: DictationScheduler {
    { delay, fire in
      let timer = Timer(delay: delay, fire: fire)
      self.timers.append(timer)
      return timer
    }
  }

  func firePending() {
    guard let timer = pending else { return }
    timer.cancelled = true
    timer.fire()
  }
}

@MainActor private func makeController() -> (
  DictationController, FakeRecognizer, ManualTimers, () -> [String]
) {
  let recognizer = FakeRecognizer()
  let timers = ManualTimers()
  let controller = DictationController(
    makeRecognizer: { recognizer }, schedule: timers.scheduler)
  var transcripts: [String] = []
  controller.onTranscript = { transcripts.append($0) }
  return (controller, recognizer, timers, { transcripts })
}

/// Lets the authorization task run.
@MainActor private func settle(_ controller: DictationController) async {
  for _ in 0..<20 where controller.state == .authorizing { await Task.yield() }
}

@MainActor @Test func nothingTouchesTheRecognizerUntilTheFirstTap() {
  var made = 0
  _ = DictationController(makeRecognizer: {
    made += 1
    return FakeRecognizer()
  })
  #expect(made == 0)
}

@MainActor @Test func aSecondTapStopsAndInsertsTheFinalTranscript() async {
  let (controller, recognizer, _, transcripts) = makeController()
  controller.toggle()
  #expect(controller.state == .authorizing)
  await settle(controller)
  #expect(controller.state == .recording(""))
  #expect(controller.isRecording)

  recognizer.emit?(.partial("run the"))
  #expect(controller.state == .recording("run the"))
  controller.toggle()
  #expect(controller.state == .finishing("run the"))
  #expect(recognizer.finished == 1)
  recognizer.emit?(.final("run the tests"))
  #expect(controller.state == .idle)
  #expect(transcripts() == ["run the tests"])
}

@MainActor @Test func silenceAfterSpeechEndsTheDictation() async {
  let (controller, recognizer, timers, transcripts) = makeController()
  controller.toggle()
  await settle(controller)
  #expect(timers.pending?.delay == DictationController.noSpeechTimeout)
  recognizer.emit?(.partial("hello"))
  #expect(timers.pending?.delay == DictationController.silenceTimeout)

  timers.firePending()
  #expect(controller.state == .finishing("hello"))
  #expect(timers.pending?.delay == DictationController.finishTimeout)
  // The recognizer never reports a final result: the last partial is inserted.
  timers.firePending()
  #expect(controller.state == .idle)
  #expect(transcripts() == ["hello"])
}

@MainActor @Test func nothingSaidStopsWithoutInserting() async {
  let (controller, _, timers, transcripts) = makeController()
  controller.toggle()
  await settle(controller)
  timers.firePending()
  timers.firePending()
  #expect(controller.state == .idle)
  #expect(transcripts().isEmpty)
}

@MainActor @Test func aRefusedPermissionExplainsAndTheNextTapAsksAgain() async {
  let (controller, recognizer, _, _) = makeController()
  recognizer.access = .denied("Allow the microphone")
  controller.toggle()
  await settle(controller)
  #expect(controller.state == .failed("Allow the microphone"))
  #expect(controller.failure == "Allow the microphone")

  recognizer.access = .authorized
  controller.toggle()
  await settle(controller)
  #expect(recognizer.authorizations == 2)
  #expect(controller.isRecording)
}

@MainActor @Test func recognizerErrorsEndInFailed() async {
  let (controller, recognizer, timers, transcripts) = makeController()
  controller.toggle()
  await settle(controller)
  recognizer.emit?(.failed("Audio engine stopped"))
  #expect(controller.state == .failed("Audio engine stopped"))
  #expect(timers.pending == nil)
  #expect(transcripts().isEmpty)
}

@MainActor @Test func cancelDropsLateEvents() async {
  let (controller, recognizer, _, transcripts) = makeController()
  controller.toggle()
  await settle(controller)
  let late = recognizer.emit
  controller.cancel()
  late?(.final("too late"))
  #expect(controller.state == .idle)
  #expect(recognizer.cancelled == 1)
  #expect(transcripts().isEmpty)
}

@Test func transcriptGoesInAtTheCursorWithOneSpaceAroundIt() {
  #expect(DictationInsertion.insert("tests", into: "", at: nil) == ("tests", 5))
  #expect(DictationInsertion.insert("the tests", into: "Run", at: nil) == ("Run the tests", 13))
  let middle = DictationInsertion.insert("all", into: "Run tests", at: 4)
  #expect(middle.text == "Run all tests")
  #expect(middle.cursor == 7)
  #expect(DictationInsertion.insert("now", into: "Run tests.", at: 9).text == "Run tests now.")
  #expect(DictationInsertion.insert("x", into: "ab", at: 99).text == "ab x")
}
