@preconcurrency import AVFoundation
import Foundation

/// Converts microphone buffers to the mono float samples an engine's model expects. It keeps
/// the converter between calls, so one audio thread at a time may use it.
public final class AudioResampler: @unchecked Sendable {
  public enum Failure: LocalizedError {
    case unsupportedFormat

    public var errorDescription: String? { "The microphone audio could not be converted." }
  }

  private let target: AVAudioFormat
  private var converter: AVAudioConverter?

  /// `sampleRate` is the rate of the model's front end.
  public init(sampleRate: Double) throws {
    guard
      let target = AVAudioFormat(
        commonFormat: .pcmFormatFloat32, sampleRate: sampleRate, channels: 1, interleaved: false)
    else { throw Failure.unsupportedFormat }
    self.target = target
  }

  public func samples(from buffer: AVAudioPCMBuffer) throws -> [Float] {
    let converted = try convert(buffer)
    guard let channel = converted.floatChannelData?[0], converted.frameLength > 0 else {
      return []
    }
    return Array(UnsafeBufferPointer(start: channel, count: Int(converted.frameLength)))
  }

  private func convert(_ buffer: AVAudioPCMBuffer) throws -> AVAudioPCMBuffer {
    if buffer.format == target { return buffer }
    if converter?.inputFormat != buffer.format {
      converter = AVAudioConverter(from: buffer.format, to: target)
      converter?.primeMethod = .none
    }
    guard let converter else { throw Failure.unsupportedFormat }
    let ratio = target.sampleRate / buffer.format.sampleRate
    let capacity = AVAudioFrameCount((Double(buffer.frameLength) * ratio).rounded(.up))
    guard let output = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
      throw Failure.unsupportedFormat
    }
    // The converter calls the block synchronously, on this thread, before `convert` returns.
    nonisolated(unsafe) var handedOver = false
    var failure: NSError?
    let status = converter.convert(to: output, error: &failure) { _, inputStatus in
      defer { handedOver = true }
      inputStatus.pointee = handedOver ? .noDataNow : .haveData
      return handedOver ? nil : buffer
    }
    if status == .error { throw failure ?? Failure.unsupportedFormat }
    return output
  }
}
