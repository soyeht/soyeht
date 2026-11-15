import Foundation
import Testing

@testable import SoyehtClient

private func wav(
  tag: UInt16, channels: UInt16, bits: UInt16, rate: UInt32, body: [UInt8], declared: UInt32? = nil
) -> Data {
  func le<V: FixedWidthInteger>(_ value: V) -> [UInt8] {
    withUnsafeBytes(of: value.littleEndian) { Array($0) }
  }
  var bytes = Array("RIFF".utf8) + le(UInt32(36 + body.count)) + Array("WAVEfmt ".utf8)
  bytes += le(UInt32(16)) + le(tag) + le(channels) + le(rate)
  bytes += le(rate &* UInt32(channels * bits / 8)) + le(channels * bits / 8) + le(bits)
  bytes += Array("data".utf8) + le(declared ?? UInt32(body.count)) + body
  return Data(bytes)
}

@Test func aRecordingSurvivesTheRoundTripToAWaveFile() throws {
  let samples: [Float] = [0, 0.5, -0.5, 1, -1, 0.25]
  let speech = try WaveAudio.decode(WaveAudio.encode(samples, sampleRate: 16_000))
  #expect(speech.sampleRate == 16_000)
  #expect(speech.samples.count == samples.count)
  for (heard, sent) in zip(speech.samples, samples) { #expect(abs(heard - sent) < 0.001) }
}

@Test func theEncodedFileHasAStandardHeader() {
  let data = WaveAudio.encode([0, 0], sampleRate: 16_000)
  #expect(data.count == 44 + 4)
  #expect(String(decoding: data.prefix(4), as: UTF8.self) == "RIFF")
  #expect(String(decoding: data[8..<12], as: UTF8.self) == "WAVE")
}

@Test func aSampleBeyondFullScaleIsClamped() throws {
  let speech = try WaveAudio.decode(WaveAudio.encode([2, -2], sampleRate: 8_000))
  #expect(speech.samples.allSatisfy { abs($0) <= 1 })
}

@Test func aFloatFileAndItsFirstChannelAreRead() throws {
  let frames: [Float] = [0.5, -0.25, 0.125, 0.0]
  let body = frames.flatMap { value in
    withUnsafeBytes(of: value.bitPattern.littleEndian) { Array($0) }
  }
  let speech = try WaveAudio.decode(wav(tag: 3, channels: 2, bits: 32, rate: 24_000, body: body))
  #expect(speech.sampleRate == 24_000)
  #expect(speech.samples == [0.5, 0.125])
}

@Test func aStreamedFileThatDeclaresNoSizeIsReadToItsEnd() throws {
  let body: [UInt8] = [0x00, 0x40, 0x00, 0xC0]
  let speech = try WaveAudio.decode(
    wav(tag: 1, channels: 1, bits: 16, rate: 24_000, body: body, declared: UInt32.max))
  #expect(speech.samples == [0.5, -0.5])
}

@Test func audioThatIsNotAReadableWaveFileIsRefusedWithAReason() {
  #expect(throws: SpeechSynthesisError.unsupportedAudio("not a WAV file")) {
    try WaveAudio.decode(Data("ID3\u{3}".utf8))
  }
  #expect(throws: SpeechSynthesisError.unsupportedAudio("format 85 with 16 bits")) {
    try WaveAudio.decode(wav(tag: 85, channels: 1, bits: 16, rate: 16_000, body: [0, 0]))
  }
  #expect(throws: SpeechSynthesisError.noAudio) {
    try WaveAudio.decode(wav(tag: 1, channels: 1, bits: 16, rate: 16_000, body: []))
  }
  #expect(throws: SpeechSynthesisError.self) { try WaveAudio.decode(Data()) }
}

@Test func aSampleRateOutsideWhatSpeechUsesIsRefused() {
  for rate in [UInt32(0), 100, 7_999, 192_001, 4_000_000_000] {
    #expect(throws: SpeechSynthesisError.unsupportedAudio("invalid format")) {
      try WaveAudio.decode(wav(tag: 1, channels: 1, bits: 16, rate: rate, body: [0, 0]))
    }
  }
}

@Test func aFloatThatIsNotANumberBecomesSilenceAndOneBeyondFullScaleIsClamped() throws {
  let frames: [Float] = [.nan, .infinity, -.infinity, 7, -7, 0.25]
  let body = frames.flatMap { value in
    withUnsafeBytes(of: value.bitPattern.littleEndian) { Array($0) }
  }
  let speech = try WaveAudio.decode(wav(tag: 3, channels: 1, bits: 32, rate: 24_000, body: body))
  #expect(speech.samples == [0, 0, 0, 1, -1, 0.25])
}

@Test func aSampleThatIsNotANumberIsNotSent() throws {
  let speech = try WaveAudio.decode(
    WaveAudio.encode([.nan, .infinity, 0.5], sampleRate: 16_000))
  #expect(speech.samples[0] == 0)
  #expect(speech.samples[1] == 0)
  #expect(abs(speech.samples[2] - 0.5) < 0.001)
}

@Test func aHeaderThatPromisesMoreThanTheFileHoldsIsReadToItsEnd() throws {
  // 100 bytes declared, 2 present.
  let speech = try WaveAudio.decode(
    wav(tag: 1, channels: 1, bits: 16, rate: 24_000, body: [0, 0x40], declared: 100))
  #expect(speech.samples == [0.5])
}
