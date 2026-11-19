import Foundation

/// The audio a voice loop runs on: the microphone, with the system's echo cancellation, and the
/// speaker. The controller uses this interface; tests use a fake.
@MainActor public protocol VoiceAudio: AnyObject {
  /// Called, once, when the audio stops by itself: a device was unplugged, a call interrupted it,
  /// the system reset its audio services. The loop ends with the reason.
  var onFailure: (@MainActor (String) -> Void)? { get set }
  /// Starts the microphone. `onSamples` gets mono samples at 16 kHz, in order, from the audio thread.
  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws
  func stop()
  /// Runs `start`, which opens a recognizer's microphone: the recognizer joins the loop's audio,
  /// beginning at `milliseconds` of the microphone's clock so the first word of a turn is not lost
  /// to the time it took to notice it. A capture that joins at any other time is not the loop's (a
  /// dictation).
  func listening(from milliseconds: Int, _ start: () throws -> Void) rethrows
  /// A capture that the loop did not start (a dictation) is joined: the loop does not take what it
  /// hears for the person's turn.
  var foreignCaptureActive: Bool { get }
  /// When, on the microphone's clock in milliseconds, the last such capture left.
  var foreignLeftAt: Int { get }
  /// Plays the speech and returns when it has been heard, or when `stopPlayback` cut it short.
  func play(_ speech: SynthesizedSpeech) async throws
  func stopPlayback()
  /// Seconds of the speech being played that have been heard, from the audio clock.
  var playbackPosition: Double { get }
}
