import Foundation

/// The WAV container, the one audio format the cloud speech endpoints share: the recording that
/// goes out as 16-bit PCM, and the speech that comes back as 16-bit PCM or 32-bit float.
package enum WaveAudio {
  /// The rate a recording is sent at: what the Whisper-class models are trained on.
  package static let recordingSampleRate = 16_000
  /// The rates a provider's speech may have; anything else is not speech this client plays.
  package static let speechSampleRates = 8_000...192_000

  /// A 16-bit mono PCM file with `samples`, clamped to -1...1.
  package static func encode(_ samples: [Float], sampleRate: Int) -> Data {
    var data = Data()
    let payload = samples.count * 2
    data.reserveCapacity(44 + payload)
    data.append(contentsOf: Array("RIFF".utf8))
    data.append(littleEndian: UInt32(36 + payload))
    data.append(contentsOf: Array("WAVEfmt ".utf8))
    data.append(littleEndian: UInt32(16))
    data.append(littleEndian: UInt16(1))
    data.append(littleEndian: UInt16(1))
    data.append(littleEndian: UInt32(sampleRate))
    data.append(littleEndian: UInt32(sampleRate * 2))
    data.append(littleEndian: UInt16(2))
    data.append(littleEndian: UInt16(16))
    data.append(contentsOf: Array("data".utf8))
    data.append(littleEndian: UInt32(payload))
    for sample in samples {
      let level = sample.isFinite ? max(-1, min(1, sample)) : 0
      data.append(littleEndian: UInt16(bitPattern: Int16((level * 32_767).rounded())))
    }
    return data
  }

  /// Mono float samples of a PCM (16-bit) or IEEE float (32-bit) file; the first channel of a
  /// file with more. A sample that is not a number becomes silence and one beyond full scale is
  /// clamped. Throws `unsupportedAudio` for anything else, for example a compressed format a
  /// provider returned although WAV was asked for, or a sample rate outside `speechSampleRates`.
  /// The file is read in place: the only copy is the samples.
  package static func decode(_ data: Data) throws -> SynthesizedSpeech {
    try data.withUnsafeBytes { bytes in
      guard bytes.count >= 12, bytes.starts(with: "RIFF".utf8),
        bytes[8..<12].elementsEqual("WAVE".utf8)
      else { throw SpeechSynthesisError.unsupportedAudio("not a WAV file") }
      var format: Format?
      var offset = 12
      while offset + 8 <= bytes.count {
        let name = bytes[offset..<offset + 4]
        let declared = Int(Self.uint32(bytes, offset + 4))
        let body = offset + 8
        if name.elementsEqual("fmt ".utf8), body + 16 <= bytes.count {
          format = Format(
            tag: Int(Self.uint16(bytes, body)), channels: Int(Self.uint16(bytes, body + 2)),
            sampleRate: Int(Self.uint32(bytes, body + 4)),
            bits: Int(Self.uint16(bytes, body + 14)))
        } else if name.elementsEqual("data".utf8) {
          guard let format else { throw SpeechSynthesisError.unsupportedAudio("no format") }
          // A streamed file declares a size it does not have: read to the end of the bytes.
          let end = declared == Int(UInt32.max) ? bytes.count : min(body + declared, bytes.count)
          return try format.speech(UnsafeRawBufferPointer(rebasing: bytes[body..<max(body, end)]))
        }
        offset = body + declared + (declared & 1)
      }
      throw SpeechSynthesisError.unsupportedAudio("no audio data")
    }
  }

  private struct Format {
    let tag: Int
    let channels: Int
    let sampleRate: Int
    let bits: Int

    func speech(_ body: UnsafeRawBufferPointer) throws -> SynthesizedSpeech {
      let frame = channels * bits / 8
      guard channels > 0, frame > 0, WaveAudio.speechSampleRates.contains(sampleRate) else {
        throw SpeechSynthesisError.unsupportedAudio("invalid format")
      }
      let count = body.count / frame
      let samples: [Float]
      switch (tag, bits) {
      case (1, 16):
        samples = (0..<count).map { index in
          Float(Int16(bitPattern: WaveAudio.uint16(body, index * frame))) / 32_768
        }
      case (3, 32):
        samples = (0..<count).map { index in
          let value = Float(bitPattern: WaveAudio.uint32(body, index * frame))
          return value.isFinite ? max(-1, min(1, value)) : 0
        }
      default:
        throw SpeechSynthesisError.unsupportedAudio("format \(tag) with \(bits) bits")
      }
      guard !samples.isEmpty else { throw SpeechSynthesisError.noAudio }
      return SynthesizedSpeech(samples: samples, sampleRate: sampleRate)
    }
  }

  private static func uint16(_ bytes: UnsafeRawBufferPointer, _ at: Int) -> UInt16 {
    UInt16(littleEndian: bytes.loadUnaligned(fromByteOffset: at, as: UInt16.self))
  }

  private static func uint32(_ bytes: UnsafeRawBufferPointer, _ at: Int) -> UInt32 {
    UInt32(littleEndian: bytes.loadUnaligned(fromByteOffset: at, as: UInt32.self))
  }
}

extension Data {
  fileprivate mutating func append<Value: FixedWidthInteger>(littleEndian value: Value) {
    Swift.withUnsafeBytes(of: value.littleEndian) { append(contentsOf: $0) }
  }
}
