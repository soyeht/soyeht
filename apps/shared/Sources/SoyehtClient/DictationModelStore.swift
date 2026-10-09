import Foundation
import Observation

/// What an engine tells the download plumbing while it fetches and loads its model.
public enum DictationModelStage: Equatable, Sendable {
  /// Fetching the files, `0...1`.
  case downloading(Double)
  /// Everything is on disk; the engine is compiling and loading it.
  case preparing
}

/// Downloads (when needed) and loads an engine's model. It reports its stage through `report`
/// and returns when the engine can listen. It owns the model it loaded: the engine's
/// recognizers use it.
///
/// When the model is already installed it must load it from disk and use no network: the app
/// calls it at launch for every installed model.
public typealias DictationModelInstaller =
  @Sendable (_ report: @escaping @Sendable (DictationModelStage) -> Void) async throws -> Void

/// The model an engine needs on disk before it can listen. An engine with a model is an engine
/// that requires an install: nothing downloads until the person asks for it.
public struct DictationModelInfo: Sendable {
  /// For example "about 642 MB". Shown before the person agrees to the download.
  public let sizeDescription: String
  public let licenseName: String
  public let licenseURL: URL?
  public let install: DictationModelInstaller
  /// Whether the whole model is on disk.
  public let isInstalled: @Sendable () -> Bool
  /// Deletes the downloaded files and drops the loaded model.
  public let remove: @Sendable () throws -> Void

  public init(
    sizeDescription: String, licenseName: String, licenseURL: URL? = nil,
    install: @escaping DictationModelInstaller, isInstalled: @escaping @Sendable () -> Bool,
    remove: @escaping @Sendable () throws -> Void
  ) {
    self.sizeDescription = sizeDescription
    self.licenseName = licenseName
    self.licenseURL = licenseURL
    self.install = install
    self.isInstalled = isInstalled
    self.remove = remove
  }
}

public enum DictationModelPhase: Equatable, Sendable {
  case notInstalled
  case downloading(Double)
  case preparing
  case ready
  case failed(String)

  /// A download or load is under way.
  public var isBusy: Bool {
    switch self {
    case .downloading, .preparing: true
    case .notInstalled, .ready, .failed: false
    }
  }
}

/// The install state of one engine's model, shared by every engine that has one: it runs the
/// installer one at a time, keeps the progress the UI shows, lets a failed attempt retry and
/// removes the files again.
@MainActor @Observable public final class DictationModelStore {
  public private(set) var phase: DictationModelPhase = .notInstalled
  /// The model is on disk, so the engine may be chosen. It stays true after a failed load.
  public private(set) var isInstalled: Bool
  /// Why the last removal failed, until the next one succeeds.
  public private(set) var removalFailure: String?
  @ObservationIgnored private let info: DictationModelInfo
  @ObservationIgnored private var running: Task<Void, Never>?
  @ObservationIgnored private var whenReady: (@MainActor () -> Void)?

  public init(_ info: DictationModelInfo) {
    self.info = info
    isInstalled = info.isInstalled()
  }

  public var isReady: Bool { phase == .ready }

  /// The person asked for the model: downloads it when it is missing and loads it. Does
  /// nothing while an install runs or once the model is ready. `whenReady` runs after a
  /// successful install, for example to select the engine.
  public func install(whenReady: (@MainActor () -> Void)? = nil) {
    guard running == nil, phase != .ready else { return }
    self.whenReady = whenReady
    removalFailure = nil
    run(starting: .downloading(0))
  }

  /// At launch: loads a model that is already on disk. Never downloads, so it does nothing
  /// for a model that is not installed.
  public func loadIfInstalled() {
    guard isInstalled, running == nil, phase != .ready else { return }
    run(starting: .preparing)
  }

  /// Deletes the downloaded files. Not while an install runs.
  public func remove() {
    guard running == nil else { return }
    do {
      try info.remove()
      removalFailure = nil
      isInstalled = false
      phase = .notInstalled
    } catch {
      removalFailure = error.localizedDescription
    }
  }

  private func run(starting stage: DictationModelPhase) {
    phase = stage
    let install = info.install
    running = Task { [weak self] in
      do {
        try await install { stage in
          Task { @MainActor in self?.apply(stage) }
        }
        self?.finish(.ready)
      } catch {
        self?.finish(.failed(error.localizedDescription))
      }
    }
  }

  private func apply(_ stage: DictationModelStage) {
    guard running != nil else { return }
    switch stage {
    case .downloading(let fraction): phase = .downloading(min(max(fraction, 0), 1))
    case .preparing: phase = .preparing
    }
  }

  private func finish(_ result: DictationModelPhase) {
    running = nil
    phase = result
    isInstalled = info.isInstalled()
    let callback = whenReady
    whenReady = nil
    if result == .ready { callback?() }
  }
}
