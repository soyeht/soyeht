import AVFoundation
import Foundation
import Testing

@testable import SoyehtClient

/// A vault in memory. The tests never touch the person's keychain.
final class MemoryProviderKeyVault: ProviderKeyVault, @unchecked Sendable {
  private let lock = NSLock()
  private var keys: [String: String]
  private(set) var reads = 0

  init(_ keys: [String: String] = [:]) { self.keys = keys }

  func key(for provider: String) throws -> String? {
    lock.withLock {
      reads += 1
      return keys[provider]
    }
  }

  func save(_ key: String, for provider: String) throws {
    lock.withLock { keys[provider] = key }
  }

  func forget(provider: String) throws {
    lock.withLock { keys[provider] = nil }
  }
}

/// A provider's answer, given in place of the network. It records every request.
final class FakeSpeechTransport: SpeechHTTPTransport, @unchecked Sendable {
  private let lock = NSLock()
  private var recorded: [URLRequest] = []
  private let answer: @Sendable (URLRequest) throws -> (Int, Data)

  init(_ answer: @escaping @Sendable (URLRequest) throws -> (Int, Data)) { self.answer = answer }

  var requests: [URLRequest] { lock.withLock { recorded } }

  func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
    lock.withLock { recorded.append(request) }
    let (status, data) = try answer(request)
    guard let url = request.url,
      let response = HTTPURLResponse(
        url: url, statusCode: status, httpVersion: nil, headerFields: nil)
    else { throw CloudSpeechError.badResponse }
    return (data, response)
  }
}

func cloudConfiguration(
  requiresKey: Bool = true, base: String = "https://speech.example.com/v1"
) -> CloudSpeechConfiguration {
  CloudSpeechConfiguration(
    id: "cloud.example", name: "Example speech", baseURL: URL(string: base)!,
    transcriptionModel: "listen-1", speechModel: "speak-1", voices: ["river", "stone"],
    requiresKey: requiresKey)
}

/// A mono 16 kHz buffer of `count` frames of silence, as the microphone tap would deliver.
func silentBuffer(frames count: Int = 1_600) throws -> AVAudioPCMBuffer {
  let format = AVAudioFormat(
    commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false)
  guard let format,
    let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count))
  else { throw CloudSpeechError.badResponse }
  buffer.frameLength = AVAudioFrameCount(count)
  return buffer
}
