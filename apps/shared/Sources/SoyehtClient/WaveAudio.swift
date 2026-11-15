import Foundation

/// The WAV container, the one audio format the cloud speech endpoints share: the recording that
/// goes out as 16-bit PCM, and the speech that comes back as 16-bit PCM or 32-bit float.
enum WaveAudio {
  /// The rate a recording is sent at: what the Whisper-class models are trained on.
  static let recordingSampleRate = 16_000

  /// A 16-bit mono PCM file with `samples`, clamped to -1...1.
  static func encode(_ samples: [Float], sampleRate: Int) -> Data {
    var data = Data()
    let payload = samples.count * 2
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
      data.append(
        littleEndian: UInt16(bitPattern: Int16((max(-1, min(1, sample)) * 32_767).rounded())))
    }
    return data
  }

  /// Mono float samples of a PCM (16-bit) or IEEE float (32-bit) file; the first channel of a
  /// file with more. Throws `unsupportedAudio` for anything else, for example a compressed
  /// format a provider returned although WAV was asked for.
  static func decode(_ data: Data) throws -> SynthesizedSpeech {
    let bytes = [UInt8](data)
    guard bytes.count >= 12, bytes[0..<4].elementsEqual("RIFF".utf8),
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
        return try format.speech(bytes[body..<max(body, end)])
      }
      offset = body + declared + (declared & 1)
    }
    throw SpeechSynthesisError.unsupportedAudio("no audio data")
  }

  private struct Format {
    let tag: Int
    let channels: Int
    let sampleRate: Int
    let bits: Int

    func speech(_ body: ArraySlice<UInt8>) throws -> SynthesizedSpeech {
      let frame = channels * bits / 8
      guard channels > 0, sampleRate > 0, frame > 0 else {
        throw SpeechSynthesisError.unsupportedAudio("invalid format")
      }
      let bytes = Array(body)
      let count = bytes.count / frame
      var samples = [Float](repeating: 0, count: count)
      switch (tag, bits) {
      case (1, 16):
        for index in 0..<count {
          let raw = Int16(bitPattern: WaveAudio.uint16(bytes, index * frame))
          samples[index] = Float(raw) / 32_768
        }
      case (3, 32):
        for index in 0..<count {
          samples[index] = Float(bitPattern: WaveAudio.uint32(bytes, index * frame))
        }
      default:
        throw SpeechSynthesisError.unsupportedAudio("format \(tag) with \(bits) bits")
      }
      guard !samples.isEmpty else { throw SpeechSynthesisError.noAudio }
      return SynthesizedSpeech(samples: samples, sampleRate: sampleRate)
    }
  }

  private static func uint16<Bytes: RandomAccessCollection>(_ bytes: Bytes, _ at: Int) -> UInt16
  where Bytes.Element == UInt8, Bytes.Index == Int {
    UInt16(bytes[at]) | UInt16(bytes[at + 1]) << 8
  }

  private static func uint32<Bytes: RandomAccessCollection>(_ bytes: Bytes, _ at: Int) -> UInt32
  where Bytes.Element == UInt8, Bytes.Index == Int {
    UInt32(bytes[at]) | UInt32(bytes[at + 1]) << 8 | UInt32(bytes[at + 2]) << 16
      | UInt32(bytes[at + 3]) << 24
  }
}

extension Data {
  fileprivate mutating func append<Value: FixedWidthInteger>(littleEndian value: Value) {
    Swift.withUnsafeBytes(of: value.littleEndian) { append(contentsOf: $0) }
  }
}
