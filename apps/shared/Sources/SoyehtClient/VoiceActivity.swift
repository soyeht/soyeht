import Foundation
import SoyehtProtocol

/// The audio the voice-activity model and the end-of-turn model read: mono floats at 16 kHz, in
/// frames of 512 samples (32 ms), the size of the models' own windows (`x-clientPolicy.voice`).
public enum VoiceFrames {
  public static let sampleRate = ClientPolicy.voiceSampleRate
  public static let size = sampleRate * ClientPolicy.voiceFrameMs / 1_000

  public static var frameMs: Int { ClientPolicy.voiceFrameMs }
}

/// A voice-activity model: how likely one frame of audio is speech. It keeps state from frame to
/// frame, so one instance serves one stream. The models of the catalogue
/// (`VoiceModelProvider`) implement it, and one needs no download: the loudness of the frame.
public protocol VoiceActivityDetecting: Sendable {
  /// The probability, 0 to 1, that `frame` (`VoiceFrames.size` samples) holds speech. A model that
  /// cannot answer throws: that is not a silence.
  func probability(of frame: [Float]) throws -> Double
  /// Forgets the stream so far.
  func reset()
}

/// An end-of-turn model: whether the person has finished what they were saying, read from the sound
/// of the end of it (its pace and its fall in pitch) rather than from a silence alone.
public protocol EndOfTurnDetecting: Sendable {
  /// `audio` is the last `turnWindowSeconds` (8) seconds or fewer of the turn, at 16 kHz. A model
  /// that cannot answer throws, and the loop ends the turn at the pause.
  func isComplete(turn audio: [Float]) async throws -> Bool
}
