import Foundation
import Observation

/// What the keyboard shortcut did.
public enum DictationCycleResult: Equatable, Sendable {
  /// The next installed engine is selected.
  case switched
  /// Only one engine is installed and another needs an install first: open Preferences at the
  /// install step. Nothing was downloaded.
  case needsInstall
  case nothingToSwitch
}

/// The person's dictation choices on this device, over the catalogue of engines: which engine,
/// which language, and the vocabulary. Preferences, the shortcut, the badge and the composer's
/// mic all read this store; none of them knows an engine by name.
///
/// Apple's engine is the default. An engine that needs a model is opt-in: nothing downloads
/// until the person presses Install for it, and it cannot be selected before it is installed.
@MainActor @Observable public final class DictationEngineStore {
  /// One row of the engine list in Preferences.
  public struct Choice: Identifiable {
    public let engine: DictationEngine
    /// Why the engine cannot run on this device, or nil.
    public let unavailableReason: String?
    /// The install state, for an engine that requires an install.
    public let model: DictationModelStore?
    public let isSelected: Bool
    /// The shortcut asked for this engine, which is not installed: show its install step.
    public let isInstallPrompt: Bool

    /// It runs here but its model is not on disk.
    public let needsInstall: Bool

    public var id: String { engine.id }
    public var isSelectable: Bool { unavailableReason == nil && !needsInstall }
  }

  /// What the app uses until it installs its own catalogue; Apple's engine only.
  public static let shared = DictationEngineStore()

  public private(set) var registry: DictationEngineRegistry
  public let vocabulary: DictationVocabularyStore
  /// When the engine last changed by the shortcut; the mic shows the new engine's badge.
  public private(set) var switchedAt: Date?
  /// The engine the shortcut wanted but could not use because it is not installed.
  public private(set) var installPrompt: String?

  private var models: [String: DictationModelStore] = [:]
  private var storedEngine: String?
  private var storedLanguage: String?
  @ObservationIgnored private let defaults: UserDefaults
  @ObservationIgnored private let platform: DictationPlatform

  public init(
    registry: DictationEngineRegistry = .appleOnly, defaults: UserDefaults = .standard,
    vocabulary: DictationVocabularyStore? = nil, platform: DictationPlatform = .current
  ) {
    self.registry = registry
    self.defaults = defaults
    self.vocabulary = vocabulary ?? DictationVocabularyStore(defaults: defaults)
    self.platform = platform
    storedEngine = defaults.string(forKey: Self.engineKey)
    storedLanguage = defaults.string(forKey: Self.languageKey)
    models = Self.modelStores(for: registry)
  }

  private static let engineKey = "soyeht.dictation.engine"
  private static let languageKey = "soyeht.dictation.language"

  private static func modelStores(for registry: DictationEngineRegistry) -> [String:
    DictationModelStore]
  {
    Dictionary(
      uniqueKeysWithValues: registry.engines.compactMap { engine in
        engine.model.map { (engine.id, DictationModelStore($0)) }
      })
  }

  /// Replaces the catalogue, as the app does once at launch, and loads the models that are
  /// already installed. It never downloads one.
  public func install(_ registry: DictationEngineRegistry) {
    self.registry = registry
    models = Self.modelStores(for: registry)
    for model in models.values { model.loadIfInstalled() }
  }

  // MARK: Engine

  /// The engine the person chose, or the fallback when it cannot run on this device or its
  /// model is not installed.
  public var selected: DictationEngine {
    registry.engine(id: selectedID) ?? registry.fallback
  }

  private var selectedID: String {
    guard let id = storedEngine, isSelectable(id) else { return registry.fallback.id }
    return id
  }

  /// The engine that listens now: the selected one once its model is ready, the fallback until
  /// then, so dictation keeps working while a model loads or after a failed load.
  public var active: DictationEngine {
    let choice = selected
    guard let model = models[choice.id], !model.isReady else { return choice }
    return registry.fallback
  }

  /// Why the mic uses `active` instead of `selected`, or nil when it does not.
  public var fallbackNote: String? {
    let choice = selected
    guard let model = models[choice.id], !model.isReady else { return nil }
    let fallback = registry.fallback.name
    switch model.phase {
    case .downloading, .preparing:
      return "\(choice.name) is loading. \(fallback) dictates until it is ready."
    case .failed(let reason):
      return "\(choice.name) could not load: \(reason) \(fallback) dictates instead."
    case .notInstalled, .ready: return "\(choice.name) is not ready. \(fallback) dictates."
    }
  }

  public var choices: [Choice] {
    registry.engines.map { engine in
      let reason = engine.unavailableReason(on: platform)
      let model = models[engine.id]
      return Choice(
        engine: engine, unavailableReason: reason, model: model,
        isSelected: engine.id == selectedID, isInstallPrompt: engine.id == installPrompt,
        needsInstall: reason == nil && model?.isInstalled == false)
    }
  }

  /// The engine runs on this device and, when it has a model, the model is installed.
  private func isSelectable(_ id: String) -> Bool {
    guard let engine = registry.engine(id: id) else { return false }
    return engine.unavailableReason(on: platform) == nil && models[id]?.isInstalled != false
  }

  public func model(for engine: DictationEngine) -> DictationModelStore? {
    models[engine.id]
  }

  public func select(_ id: String) {
    guard isSelectable(id) else { return }
    installPrompt = nil
    storedEngine = id
    defaults.set(id, forKey: Self.engineKey)
  }

  /// The person pressed Install for `id`: downloads its model (the only way a download starts)
  /// and selects the engine once it is ready.
  public func install(engineID id: String) {
    guard let engine = registry.engine(id: id), engine.unavailableReason(on: platform) == nil
    else { return }
    installPrompt = nil
    models[id]?.install { [weak self] in self?.select(id) }
  }

  /// Deletes the engine's downloaded model. If it was the selected engine, Apple's takes over.
  public func remove(engineID id: String) {
    guard let model = models[id] else { return }
    model.remove()
    guard !model.isInstalled, storedEngine == id else { return }
    storedEngine = nil
    defaults.removeObject(forKey: Self.engineKey)
  }

  public func dismissInstallPrompt() {
    installPrompt = nil
  }

  /// The shortcut: the next installed engine this device can run, wrapping around. With no
  /// second installed engine it points at an engine that needs an install instead, and never
  /// starts a download.
  @discardableResult
  public func cycle() -> DictationCycleResult {
    let usable = choices.filter(\.isSelectable)
    if usable.count > 1, let index = usable.firstIndex(where: { $0.engine.id == selected.id }) {
      select(usable[(index + 1) % usable.count].id)
      switchedAt = Date()
      return .switched
    }
    guard let pending = choices.first(where: \.needsInstall) else { return .nothingToSwitch }
    installPrompt = pending.id
    return .needsInstall
  }

  // MARK: Language

  /// The language selection shown in Preferences for the selected engine.
  public var language: DictationLanguage {
    resolvedLanguage(for: selected)
  }

  public func selectLanguage(_ language: DictationLanguage) {
    storedLanguage = language.id
    defaults.set(language.id, forKey: Self.languageKey)
  }

  /// The stored language when `engine` supports it, otherwise System default, so switching to
  /// an engine without that language never leaves a dead selection.
  private func resolvedLanguage(for engine: DictationEngine) -> DictationLanguage {
    engine.languages.first { $0.id == storedLanguage } ?? .system
  }

  // MARK: Session

  /// What the composer's mic needs for one dictation: the recognizer of the active engine, its
  /// options and the words to show.
  public func session() -> DictationSession {
    let engine = active
    let language = resolvedLanguage(for: engine)
    return DictationSession(
      recognizer: engine.make(),
      options: SpeechSessionOptions(
        language: engine.languageTag(for: language.id),
        vocabulary: engine.usesVocabulary ? vocabulary.phrases : []),
      badge: engine.badge, note: fallbackNote)
  }
}
