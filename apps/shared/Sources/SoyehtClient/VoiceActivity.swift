import Foundation
import SoyehtProtocol

/// The audio the voice-activity model and the end-of-turn model read: mono floats at 16 kHz, in
/// frames of 512 samples (32 ms), the size of the models' own windows (`x-clientPolicy.voice`).
public enum VoiceFrames {
  public static let sampleRate = ClientPolicy.voiceSampleRate
  public static let size = sampleRate * ClientPolicy.voiceFrameMs / 1_000
  /// The audio the end-of-turn model reads: the last 8 seconds of the turn.
  static let turnWindow = ClientPolicy.voiceTurnWindowSeconds * sampleRate

  public static var frameMs: Int { ClientPolicy.voiceFrameMs }
}

/// A voice-activity model: how likely one frame of audio is speech. It keeps state from frame to
/// frame, so one instance serves one stream. The models of the catalogue
/// (`VoiceModelProvider`) implement it; `EnergyVoiceActivity` is the one that needs no download.
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
  /// `audio` is the last `VoiceFrames.turnWindow` samples or fewer of the turn, at 16 kHz. A model
  /// that cannot answer throws, and the loop ends the turn at the pause.
  func isComplete(turn audio: [Float]) async throws -> Bool
}

/// The end of turn with no model: a pause long enough (the turn machine's `silenceMs`) is the end.
public struct SilenceEndOfTurn: EndOfTurnDetecting {
  public init() {}

  public func isComplete(turn audio: [Float]) async throws -> Bool { true }
}

/// Voice activity from loudness alone: a frame is speech when it is clearly louder than the quietest
/// the room has been lately. It is the floor of the catalogue: it needs no model, and it takes any
/// loud noise for speech, which the recognizer then answers with no words.
public final class EnergyVoiceActivity: VoiceActivityDetecting, @unchecked Sendable {
  /// How many decibels above the room a frame must be to count at all, and to count fully.
  static let marginDb = 6.0
  static let spanDb = 12.0
  /// The estimate of the room rises this much per frame, so it follows a room that gets louder but
  /// not a person who keeps talking.
  static let riseDb = 0.05
  /// Below this level nothing is speech, whatever the room.
  static let silenceDb = -60.0

  private let lock = NSLock()
  private var floorDb: Double?

  public init() {}

  public func probability(of frame: [Float]) -> Double {
    let level = Self.decibels(of: frame)
    return lock.withLock {
      let floor = min(level, (floorDb ?? level) + Self.riseDb)
      floorDb = floor
      guard level > Self.silenceDb else { return 0 }
      return min(max((level - floor - Self.marginDb) / Self.spanDb, 0), 1)
    }
  }

  public func reset() { lock.withLock { floorDb = nil } }

  static func decibels(of frame: [Float]) -> Double {
    20 * log10(max(Double(SpeechEnvelope.rms(of: frame[...])), 1e-6))
  }
}

/// How loud speech is at a moment, for what draws or drives the avatar's mouth.
public enum SpeechEnvelope {
  /// The root-mean-square of the `windowSeconds` of `samples` that start `position` seconds in,
  /// 0 to 1 for samples in -1 to 1; 0 outside the audio.
  public static func level(
    of samples: [Float], sampleRate: Int, at position: Double, windowSeconds: Double = 0.032
  ) -> Float {
    guard sampleRate > 0, position >= 0, position.isFinite else { return 0 }
    let start = Int(position * Double(sampleRate))
    let end = min(samples.count, start + max(1, Int(windowSeconds * Double(sampleRate))))
    guard start < end else { return 0 }
    return rms(of: samples[start..<end])
  }

  static func rms(of samples: ArraySlice<Float>) -> Float {
    guard !samples.isEmpty else { return 0 }
    return (samples.reduce(0) { $0 + $1 * $1 } / Float(samples.count)).squareRoot()
  }
}
