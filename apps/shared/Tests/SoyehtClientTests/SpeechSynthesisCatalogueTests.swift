import AVFoundation
import Foundation
import Testing

@testable import SoyehtClient

private struct FakeSynthesizer: SpeechSynthesizing {
  let name: String

  func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
  {
    SynthesizedSpeech(samples: [0.1, 0.2], sampleRate: 8_000)
  }
}

private func fakeProvider(
  _ id: String, model: DictationModelInfo? = nil, cloud: CloudProviderInfo? = nil,
  platforms: Set<DictationPlatform> = Set(DictationPlatform.allCases),
  availability: DictationAvailability = .available
) -> SpeechSynthesisProvider {
  SpeechSynthesisProvider(
    id: id, name: "Provider \(id)", summary: "Fake \(id)", platforms: platforms,
    voices: { [SpeechVoice(id: "\(id)-voice", name: "Voice", language: "en-US")] },
    model: model, cloud: cloud, availability: { availability },
    make: { FakeSynthesizer(name: id) })
}

/// A model on a fake disk: nothing downloads until `install` runs.
private final class FakeVoiceModel: @unchecked Sendable {
  private let lock = NSLock()
  private var downloads = 0
  private var installed = false

  var downloadCount: Int { lock.withLock { downloads } }

  var info: DictationModelInfo {
    DictationModelInfo(
      sizeDescription: "about 1 MB", licenseName: "Apache-2.0", commercialUse: true,
      install: { _ in
        self.lock.withLock {
          self.downloads += 1
          self.installed = true
        }
      },
      isInstalled: { self.lock.withLock { self.installed } },
      remove: { self.lock.withLock { self.installed = false } })
  }
}

@Test func theSpeechFallbackStaysFirstAndDuplicateIdsAreIgnored() {
  let registry = SpeechSynthesisRegistry(
    fallback: fakeProvider("a"), others: [fakeProvider("b"), fakeProvider("a"), fakeProvider("b")])
  #expect(registry.providers.map(\.id) == ["a", "b"])
  #expect(registry.fallback.id == "a")
  #expect(registry.provider(id: "b")?.name == "Provider b")
  #expect(registry.provider(id: "missing") == nil)
}

/// The recipe in the ADR: a new provider is one descriptor in the registry, and the voice screen
/// reads everything from it.
@Test func aThirdProviderAppearsWithEverythingTheScreenNeedsAndNoOtherChange() {
  let registry = SpeechSynthesisRegistry(
    fallback: fakeProvider("apple"),
    others: [
      fakeProvider("local", model: FakeVoiceModel().info),
      fakeProvider("cloud", cloud: CloudProviderInfo(host: "speech.example.com", sends: [.text])),
    ])
  #expect(registry.providers.map(\.id) == ["apple", "local", "cloud"])
  #expect(registry.providers.map(\.requiresInstall) == [false, true, false])
  #expect(registry.provider(id: "cloud")?.cloud?.host == "speech.example.com")
  #expect(registry.provider(id: "cloud")?.cloud?.sends == [.text])
  #expect(registry.provider(id: "local")?.voices().map(\.id) == ["local-voice"])
}

@Test func aProviderSaysWhyItCannotSpeakHere() {
  let phoneOnly = fakeProvider("phone", platforms: [.iPhone])
  #expect(phoneOnly.unavailableReason(on: .iPhone) == nil)
  #expect(phoneOnly.unavailableReason(on: .mac) == "Not available on Mac")
  let waiting = fakeProvider("waiting", availability: .unavailable("Needs a key"))
  #expect(waiting.unavailableReason(on: .mac) == "Needs a key")
}

@MainActor @Test func aProviderModelIsInstalledByTheDictationInstallerOnlyWhenAsked() async {
  let model = FakeVoiceModel()
  let provider = fakeProvider("local", model: model.info)
  guard let info = provider.model else {
    Issue.record("the provider has a model")
    return
  }
  let store = DictationModelStore(info)
  #expect(store.phase == .notInstalled)
  #expect(!store.isInstalled)
  #expect(model.downloadCount == 0)
  store.install()
  for _ in 0..<200 where !store.isReady { await Task.yield() }
  #expect(store.isReady)
  #expect(store.isInstalled)
  #expect(model.downloadCount == 1)
}

@MainActor @Test func theSystemVoicesAreTheFallbackWithNothingToInstallOrSend() {
  let apple = SpeechSynthesisProvider.apple
  #expect(apple.id == "apple")
  #expect(apple.model == nil)
  #expect(apple.cloud == nil)
  #expect(apple.platforms == Set(DictationPlatform.allCases))
  #expect(apple.unavailableReason(on: .mac) == nil)
}

@Test func theSystemRateScalesTheDefaultAndStaysInsideTheSystemRange() {
  #expect(AppleSpeechSynthesizer.rate(forSpeed: 1) == AVSpeechUtteranceDefaultSpeechRate)
  #expect(AppleSpeechSynthesizer.rate(forSpeed: 100) == AVSpeechUtteranceMaximumSpeechRate)
  #expect(AppleSpeechSynthesizer.rate(forSpeed: 0) == AVSpeechUtteranceMinimumSpeechRate)
}

@Test func theSamplesOfARenderedBufferAreReadWhicheverTypeTheSystemUsed() throws {
  let floats = try #require(
    AVAudioFormat(
      commonFormat: .pcmFormatFloat32, sampleRate: 22_050, channels: 1, interleaved: false))
  let floatBuffer = try #require(AVAudioPCMBuffer(pcmFormat: floats, frameCapacity: 3))
  floatBuffer.frameLength = 3
  floatBuffer.floatChannelData?[0].update(from: [0.5, -0.5, 0.25], count: 3)
  #expect(AppleSpeechSynthesizer.samples(of: floatBuffer) == [0.5, -0.5, 0.25])

  let ints = try #require(
    AVAudioFormat(
      commonFormat: .pcmFormatInt16, sampleRate: 22_050, channels: 1, interleaved: false))
  let intBuffer = try #require(AVAudioPCMBuffer(pcmFormat: ints, frameCapacity: 2))
  intBuffer.frameLength = 2
  intBuffer.int16ChannelData?[0].update(from: [16_384, -16_384], count: 2)
  #expect(AppleSpeechSynthesizer.samples(of: intBuffer) == [0.5, -0.5])
}

@Test func theLengthOfSpeechFollowsItsSampleRate() {
  #expect(
    SynthesizedSpeech(samples: [Float](repeating: 0, count: 12_000), sampleRate: 24_000).duration
      == .seconds(0.5))
  #expect(SynthesizedSpeech(samples: [0], sampleRate: 0).duration == .zero)
}
