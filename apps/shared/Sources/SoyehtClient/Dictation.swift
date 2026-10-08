import Foundation
import Observation

/// Voice dictation into the composer: tap the mic to listen, tap again (or stay silent) to
/// stop, and the transcript is inserted where the cursor is.
///
/// idle → authorizing → recording → finishing → idle; a refusal or an error leaves
/// `failed` with the reason, and the next tap starts over.
@MainActor @Observable public final class DictationController {
  public enum State: Equatable, Sendable {
    case idle
    case authorizing
    case recording(String)
    case finishing(String)
    case failed(String)
  }

  /// Silence after speech that ends the dictation.
  public static let silenceTimeout: Duration = .seconds(2)
  /// How long the mic listens when nothing is said at all.
  public static let noSpeechTimeout: Duration = .seconds(8)
  /// How long a stopped recognizer may take to report its final transcript.
  public static let finishTimeout: Duration = .seconds(2)

  public private(set) var state: State = .idle
  /// Receives the transcript to insert into the composer.
  @ObservationIgnored public var onTranscript: (String) -> Void = { _ in }

  @ObservationIgnored private let makeRecognizer: @MainActor () -> SpeechRecognizing
  @ObservationIgnored private let schedule: DictationScheduler
  @ObservationIgnored private var recognizer: SpeechRecognizing?
  @ObservationIgnored private var timer: DictationTimer?
  @ObservationIgnored private var session = 0

  /// `makeRecognizer` runs on the first tap, never earlier, so rendering the composer never
  /// touches the Speech framework or the privacy prompts.
  public init(
    makeRecognizer: @escaping @MainActor () -> SpeechRecognizing,
    schedule: @escaping DictationScheduler = DictationController.taskScheduler
  ) {
    self.makeRecognizer = makeRecognizer
    self.schedule = schedule
  }

  public var isRecording: Bool {
    if case .recording = state { return true }
    return false
  }

  public var isBusy: Bool {
    switch state {
    case .authorizing, .recording, .finishing: true
    case .idle, .failed: false
    }
  }

  public var failure: String? {
    if case .failed(let message) = state { return message }
    return nil
  }

  /// The mic button: start listening, or stop and insert what was heard.
  public func toggle() {
    switch state {
    case .idle, .failed: start()
    case .recording: finish()
    case .authorizing, .finishing: break
    }
  }

  /// Stops without inserting anything, for example when the pane goes away.
  public func cancel() {
    session += 1
    timer?.cancel()
    timer = nil
    recognizer?.cancel()
    state = .idle
  }

  private func start() {
    session += 1
    let current = session
    state = .authorizing
    let recognizer = self.recognizer ?? makeRecognizer()
    self.recognizer = recognizer
    Task { @MainActor in
      let access = await recognizer.authorize()
      guard current == self.session, self.state == .authorizing else { return }
      switch access {
      case .authorized: self.listen(recognizer, session: current)
      case .denied(let reason), .unavailable(let reason): self.state = .failed(reason)
      }
    }
  }

  private func listen(_ recognizer: SpeechRecognizing, session current: Int) {
    do {
      try recognizer.start { [weak self] event in
        guard let self, current == self.session else { return }
        self.receive(event)
      }
      state = .recording("")
      arm(Self.noSpeechTimeout) { $0.finish() }
    } catch {
      state = .failed(error.localizedDescription)
    }
  }

  private func receive(_ event: SpeechEvent) {
    switch event {
    case .partial(let text):
      switch state {
      case .recording:
        state = .recording(text)
        arm(Self.silenceTimeout) { $0.finish() }
      case .finishing: state = .finishing(text)
      default: break
      }
    case .final(let text): deliver(text)
    case .failed(let message):
      timer?.cancel()
      state = .failed(message)
    }
  }

  private func finish() {
    guard case .recording(let heard) = state else { return }
    state = .finishing(heard)
    recognizer?.finish()
    arm(Self.finishTimeout) { controller in
      if case .finishing(let heard) = controller.state { controller.deliver(heard) }
    }
  }

  private func deliver(_ text: String) {
    guard isBusy else { return }
    timer?.cancel()
    timer = nil
    session += 1
    state = .idle
    let transcript = text.trimmingCharacters(in: .whitespacesAndNewlines)
    if !transcript.isEmpty { onTranscript(transcript) }
  }

  private func arm(_ delay: Duration, _ fire: @escaping @MainActor (DictationController) -> Void) {
    timer?.cancel()
    let current = session
    timer = schedule(delay) { [weak self] in
      guard let self, current == self.session else { return }
      fire(self)
    }
  }

  public static let taskScheduler: DictationScheduler = { delay, fire in
    TaskTimer(delay: delay, fire: fire)
  }
}

/// A timer the controller can cancel; tests fire it by hand.
@MainActor public protocol DictationTimer: AnyObject {
  func cancel()
}

public typealias DictationScheduler =
  @MainActor (Duration, @escaping @MainActor () -> Void) -> DictationTimer

private final class TaskTimer: DictationTimer {
  private let task: Task<Void, Never>

  init(delay: Duration, fire: @escaping @MainActor () -> Void) {
    task = Task { @MainActor in
      guard (try? await Task.sleep(for: delay)) != nil else { return }
      fire()
    }
  }

  func cancel() { task.cancel() }
}

/// Puts a dictated transcript into the composer text at the cursor, with one space between
/// it and the words around it.
public enum DictationInsertion {
  /// Returns the new text and the cursor offset (in characters) after the inserted words.
  /// `cursor` is a character offset; `nil` or out of range appends at the end.
  public static func insert(
    _ transcript: String, into text: String, at cursor: Int?
  ) -> (text: String, cursor: Int) {
    let offset = min(max(cursor ?? text.count, 0), text.count)
    let index = text.index(text.startIndex, offsetBy: offset)
    let before = text[..<index]
    let after = text[index...]
    let leading = before.last.map { $0.isWhitespace ? "" : " " } ?? ""
    let trailing = after.first.map { $0.isWhitespace || $0.isPunctuation ? "" : " " } ?? ""
    let inserted = leading + transcript + trailing
    return (String(before) + inserted + String(after), offset + leading.count + transcript.count)
  }
}
