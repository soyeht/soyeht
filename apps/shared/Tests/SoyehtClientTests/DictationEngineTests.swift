import Foundation
import Testing

@testable import SoyehtClient

@MainActor private final class StubRecognizer: SpeechRecognizing {
  let engineID: String
  private(set) var started: SpeechSessionOptions?

  init(engineID: String) { self.engineID = engineID }

  func authorize() async -> DictationAuthorization { .authorized }

  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    started = options
  }

  func finish() {}
  func cancel() {}
}

/// A fake engine. Everything the app needs to know about it is in this one descriptor, the same
/// as a real engine's entry in the catalogue.
@MainActor private func fakeEngine(
  _ id: String, platforms: Set<DictationPlatform> = Set(DictationPlatform.allCases),
  languages: [String] = [], usesVocabulary: Bool = false, model: DictationModelInfo? = nil,
  availability: DictationAvailability = .available
) -> DictationEngine {
  DictationEngine(
    id: id, name: "Engine \(id)", badge: id.uppercased(), summary: "Fake \(id)",
    platforms: platforms, languages: [.system] + languages.map(DictationLanguage.init(tag:)),
    usesVocabulary: usesVocabulary, model: model, availability: { availability },
    make: { StubRecognizer(engineID: id) })
}

@MainActor private func makeStore(
  _ engines: [DictationEngine], platform: DictationPlatform = .mac
) -> DictationEngineStore {
  let defaults = UserDefaults(suiteName: "dictation-tests-\(UUID().uuidString)") ?? .standard
  return DictationEngineStore(
    registry: DictationEngineRegistry(fallback: engines[0], others: Array(engines.dropFirst())),
    defaults: defaults, platform: platform)
}

@MainActor private func engineID(of session: DictationSession) -> String? {
  (session.recognizer as? StubRecognizer)?.engineID
}

/// Lets the main actor run the tasks the model store started.
@MainActor private func settle(until condition: () -> Bool) async {
  for _ in 0..<200 where !condition() { await Task.yield() }
}

// MARK: Catalogue

@MainActor @Test func theFallbackStaysFirstAndDuplicateIdsAreIgnored() {
  let registry = DictationEngineRegistry(
    fallback: fakeEngine("a"), others: [fakeEngine("b"), fakeEngine("a"), fakeEngine("b")])
  #expect(registry.engines.map(\.id) == ["a", "b"])
  #expect(registry.fallback.id == "a")
  #expect(DictationEngineRegistry.appleOnly.fallback.id == DictationEngine.apple.id)
}

/// The recipe in the ADR: a new engine is one descriptor in the registry. Nothing else changes:
/// Preferences lists it, it can be picked, the shortcut reaches it, and the mic listens with it.
@MainActor @Test func aThirdEngineAppearsInPreferencesAndIsUsedWithNoOtherChange() {
  let store = makeStore([fakeEngine("apple"), fakeEngine("second"), fakeEngine("third")])
  #expect(store.choices.map(\.id) == ["apple", "second", "third"])
  #expect(store.choices.allSatisfy { $0.isSelectable })
  #expect(store.selected.id == "apple")

  store.select("third")
  #expect(store.choices.first { $0.isSelected }?.id == "third")
  #expect(engineID(of: store.session()) == "third")
  #expect(store.session().badge == "THIRD")

  store.cycle()
  #expect(store.selected.id == "apple")
  store.cycle()
  #expect(store.selected.id == "second")
  #expect(store.switchedAt != nil)
  _ =
    DictationPreferencesView(
      store: store,
      style: AgentConversationStyle.pane(
        SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
    ).body
}

@MainActor @Test func theChoiceIsRemembered() {
  let defaults = UserDefaults(suiteName: "dictation-tests-\(UUID().uuidString)") ?? .standard
  let registry = DictationEngineRegistry(fallback: fakeEngine("apple"), others: [fakeEngine("b")])
  DictationEngineStore(registry: registry, defaults: defaults, platform: .mac).select("b")
  #expect(
    DictationEngineStore(registry: registry, defaults: defaults, platform: .mac).selected.id == "b")
}

@MainActor @Test func anEngineThatCannotRunHereIsShownButNotSelectable() {
  let phoneOnly = fakeEngine("phone", platforms: [.iPhone])
  let old = fakeEngine("old", availability: .unavailable("Needs a newer system"))
  let store = makeStore([fakeEngine("apple"), phoneOnly, old])
  let reasons = store.choices.map(\.unavailableReason)
  #expect(reasons == [nil, "Not available on Mac", "Needs a newer system"])

  store.select("phone")
  store.select("old")
  #expect(store.selected.id == "apple")
  store.cycle()
  #expect(store.selected.id == "apple")
  #expect(makeStore([fakeEngine("apple"), phoneOnly], platform: .iPhone).choices[1].isSelectable)
}

// MARK: Languages and vocabulary

@MainActor @Test func theLanguageListFollowsTheEngineAndKeepsOnlyWhatItSupports() {
  let broad = fakeEngine("broad", languages: ["pt-BR", "en-US", "nl-NL"])
  let narrow = fakeEngine("narrow", languages: ["en-US"])
  let store = makeStore([narrow, broad])
  store.select("broad")
  store.selectLanguage(DictationLanguage(tag: "nl-NL"))
  #expect(store.language.id == "nl-NL")
  #expect(store.selected.languages.map(\.id) == ["system", "pt-BR", "en-US", "nl-NL"])
  #expect(store.session().options.language == "nl-NL")

  store.select("narrow")
  #expect(store.language == .system)
  store.select("broad")
  #expect(store.language.id == "nl-NL")
}

@MainActor @Test func systemDefaultOnlyBecomesATagTheEngineShips() {
  let engine = fakeEngine("e", languages: ["pt-BR", "en-US"])
  #expect(engine.languageTag(for: "system", systemIdentifier: "pt_BR") == "pt-BR")
  // English in Brazil is not a language the engine ships: do not guess "en".
  #expect(engine.languageTag(for: "system", systemIdentifier: "en_BR") == nil)
  #expect(engine.languageTag(for: "auto") == nil)
  #expect(engine.languageTag(for: "en-US") == "en-US")
}

@MainActor @Test func theVocabularyReachesOnlyTheEnginesThatUseIt() {
  let store = makeStore([fakeEngine("plain"), fakeEngine("boosted", usesVocabulary: true)])
  #expect(store.vocabulary.add("  Soyeht ") == .added)
  #expect(store.vocabulary.add("Soyeht") == .duplicate)
  #expect(store.vocabulary.add("   ") == .empty)
  #expect(store.session().options.vocabulary.isEmpty)
  store.select("boosted")
  #expect(store.session().options.vocabulary == ["Soyeht"])
  store.vocabulary.remove("Soyeht")
  #expect(store.session().options.vocabulary.isEmpty)
}

@Test func theVocabularyIsTrimmedDeduplicatedAndCapped() {
  let many = (0..<300).map { "word\($0)" }
  #expect(DictationVocabulary.sanitize(many).count == DictationVocabulary.phraseLimit)
  #expect(DictationVocabulary.sanitize([" a ", "a", "", "B", "b"]) == ["a", "B", "b"])
}

@MainActor @Test func theVocabularyIsKeptBetweenLaunches() {
  let defaults = UserDefaults(suiteName: "dictation-tests-\(UUID().uuidString)") ?? .standard
  DictationVocabularyStore(defaults: defaults).add("Nemotron")
  #expect(DictationVocabularyStore(defaults: defaults).phrases == ["Nemotron"])
}

// MARK: Model install

/// A model on a fake disk. Its installer reports from a background queue, as a real downloader
/// does, then waits for the test to let it finish or fail.
private final class FakeModel: @unchecked Sendable {
  private let lock = NSLock()
  private var gate: CheckedContinuation<Void, Never>?
  private var releaseEarly = false
  private var onDisk: Bool
  private var downloadCount = 0
  private var loadCount = 0
  private var removeCount = 0
  var failNext = false

  init(installed: Bool = false) { onDisk = installed }

  var installed: Bool { lock.withLock { onDisk } }
  /// Installer runs that had to fetch the model.
  var downloads: Int { lock.withLock { downloadCount } }
  /// Installer runs that loaded a model already on disk.
  var loads: Int { lock.withLock { loadCount } }
  var removals: Int { lock.withLock { removeCount } }

  var info: DictationModelInfo {
    DictationModelInfo(
      sizeDescription: "10 MB", licenseName: "Test",
      install: { report in
        let wasInstalled = self.lock.withLock { () -> Bool in
          if self.onDisk { self.loadCount += 1 } else { self.downloadCount += 1 }
          return self.onDisk
        }
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
          DispatchQueue.global().async {
            if !wasInstalled { report(.downloading(0.4)) }
            report(.preparing)
            done.resume()
          }
        }
        await self.hold()
        if self.lock.withLock({ self.failNext }) { throw FakeModelFailure.network }
        self.lock.withLock { self.onDisk = true }
      },
      isInstalled: { self.installed },
      remove: {
        self.lock.withLock {
          self.onDisk = false
          self.removeCount += 1
        }
      })
  }

  private func hold() async {
    await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
      lock.lock()
      if releaseEarly {
        releaseEarly = false
        lock.unlock()
        continuation.resume()
      } else {
        gate = continuation
        lock.unlock()
      }
    }
  }

  func release() {
    lock.lock()
    if let gate {
      self.gate = nil
      lock.unlock()
      gate.resume()
    } else {
      releaseEarly = true
      lock.unlock()
    }
  }
}

private enum FakeModelFailure: LocalizedError {
  case network
  var errorDescription: String? { "No network." }
}

@MainActor @Test func appleIsTheDefaultAndNothingDownloadsWithoutConsent() async {
  let model = FakeModel()
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.install(store.registry)
  #expect(store.selected.id == "apple")
  #expect(store.choices[1].needsInstall)
  #expect(store.choices[1].engine.model != nil)

  // Neither choosing it, the mic, nor the shortcut starts a download.
  store.select("big")
  #expect(store.selected.id == "apple")
  #expect(engineID(of: store.session()) == "apple")
  #expect(store.cycle() == .needsInstall)
  for _ in 0..<50 { await Task.yield() }
  #expect(model.downloads == 0)
  #expect(model.loads == 0)
  #expect(store.choices[1].model?.phase == .notInstalled)
}

@MainActor @Test func installingIsExplicitAndTheEngineIsUsedOnceInstalled() async {
  let model = FakeModel()
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.install(engineID: "big")
  let state = store.choices[1].model
  await settle { state?.phase == .preparing }
  #expect(model.downloads == 1)
  #expect(store.selected.id == "apple")
  #expect(engineID(of: store.session()) == "apple")

  model.release()
  await settle { state?.isReady == true }
  #expect(state?.isInstalled == true)
  #expect(store.selected.id == "big")
  #expect(store.active.id == "big")
  #expect(engineID(of: store.session()) == "big")
  #expect(store.fallbackNote == nil)
}

@MainActor @Test func theShortcutOnlyCyclesInstalledEnginesAndPointsToTheInstallStep() async {
  let ready = FakeModel(installed: true)
  let missing = FakeModel()
  let store = makeStore([
    fakeEngine("apple"), fakeEngine("ready", model: ready.info),
    fakeEngine("missing", model: missing.info),
  ])
  store.install(store.registry)
  ready.release()
  await settle { store.choices[1].model?.isReady == true }
  #expect(ready.loads == 1)
  #expect(ready.downloads == 0)

  #expect(store.cycle() == .switched)
  #expect(store.selected.id == "ready")
  #expect(store.cycle() == .switched)
  #expect(store.selected.id == "apple")
  #expect(store.installPrompt == nil)

  // With only Apple installed, the shortcut shows the install step and downloads nothing.
  let alone = makeStore([fakeEngine("apple"), fakeEngine("missing", model: missing.info)])
  #expect(alone.cycle() == .needsInstall)
  #expect(alone.installPrompt == "missing")
  #expect(alone.choices[1].isInstallPrompt)
  alone.dismissInstallPrompt()
  #expect(alone.installPrompt == nil)
  #expect(missing.downloads == 0)
  #expect(makeStore([fakeEngine("apple")]).cycle() == .nothingToSwitch)
}

@MainActor @Test func removingTheModelFreesItAndFallsBackToApple() async {
  let model = FakeModel(installed: true)
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.install(store.registry)
  model.release()
  await settle { store.choices[1].model?.isReady == true }
  store.select("big")
  #expect(store.selected.id == "big")

  store.remove(engineID: "big")
  #expect(model.removals == 1)
  #expect(!model.installed)
  #expect(store.selected.id == "apple")
  #expect(store.choices[1].needsInstall)
  #expect(store.choices[1].model?.phase == .notInstalled)
  #expect(engineID(of: store.session()) == "apple")
  #expect(model.downloads == 0)
}

@MainActor @Test func anInstalledModelLoadsAtLaunchWithoutDownloading() async {
  let model = FakeModel(installed: true)
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.select("big")
  store.install(store.registry)
  let state = store.choices[1].model
  await settle { state?.phase == .preparing }
  #expect(store.active.id == "apple")
  #expect(store.fallbackNote?.contains("Engine big is loading") == true)
  model.release()
  await settle { state?.isReady == true }
  #expect(model.downloads == 0)
  #expect(model.loads == 1)
}

@MainActor @Test func aFailedInstallKeepsTheFallbackAndRetriesOnlyWhenAsked() async {
  let model = FakeModel()
  model.failNext = true
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.install(engineID: "big")
  let state = store.choices[1].model
  model.release()
  await settle { if case .failed = state?.phase { true } else { false } }
  #expect(state?.phase == .failed("No network."))
  #expect(state?.isInstalled == false)
  #expect(store.selected.id == "apple")

  // The mic never retries the download by itself.
  #expect(engineID(of: store.session()) == "apple")
  for _ in 0..<50 { await Task.yield() }
  #expect(model.downloads == 1)

  model.failNext = false
  model.release()
  store.install(engineID: "big")
  await settle { state?.isReady == true }
  #expect(model.downloads == 2)
  #expect(store.selected.id == "big")
}

@MainActor @Test func installingTwiceRunsTheInstallerOnce() async {
  let model = FakeModel()
  let store = makeStore([fakeEngine("apple"), fakeEngine("big", model: model.info)])
  store.install(engineID: "big")
  store.install(engineID: "big")
  await settle { store.choices[1].model?.phase == .preparing }
  model.release()
  await settle { store.choices[1].model?.isReady == true }
  #expect(model.downloads == 1)
}

// MARK: Controller

@MainActor @Test func theControllerStartsEachDictationWithTheOptionsOfTheEngineThenChosen() async {
  let store = makeStore([
    fakeEngine("apple"), fakeEngine("boosted", languages: ["pt-BR"], usesVocabulary: true),
  ])
  store.vocabulary.add("Soyeht")
  store.select("boosted")
  store.selectLanguage(DictationLanguage(tag: "pt-BR"))
  let controller = DictationController(makeSession: { store.session() })
  controller.toggle()
  await settle { controller.state != .authorizing }
  #expect(controller.isRecording)
  #expect(controller.badge == "BOOSTED")

  controller.cancel()
  store.select("apple")
  controller.toggle()
  await settle { controller.state != .authorizing }
  #expect(controller.badge == "APPLE")
  controller.cancel()
}
