import Foundation

/// The events of one recognition session, passed on only while it lasts. A recognizer that ends a
/// session itself (the audio went away, the person cancelled) reports its own reason, and the system
/// then reports a cancellation of its own: that comes after the real reason and must not replace it.
@MainActor final class RecognitionSession {
  private var number = 0

  /// Starts a session, ending the one before, and returns how its events are passed on to `onEvent`.
  func begin(
    _ onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) -> @MainActor (SpeechEvent) -> Void {
    number += 1
    let mine = number
    return { [weak self] event in
      guard self?.number == mine else { return }
      onEvent(event)
    }
  }

  /// Nothing more of the session is passed on.
  func end() { number += 1 }
}
