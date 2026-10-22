import Foundation

/// A list the app loads on its own when a screen opens, not because the person asked
/// for it. A load that is slow or fails is a state of that list, never a modal alert.
public enum BackgroundLoad: Hashable, Sendable {
  case harnesses
  case readiness
  case customHarnesses
  case defaultPermissionModes
  case catalogue(harness: String)
  case editorPane(paneId: String)

  /// What a screen says when this load failed, ahead of a retry. The engine's own
  /// reason stays available through `EngineClient.loadFailure`.
  public var failureText: String {
    switch self {
    case .harnesses: "Could not load the agent list. The engine may be busy."
    case .customHarnesses: "Could not load your custom agents."
    case .readiness: "Could not check this machine. The engine may be busy."
    case .defaultPermissionModes: "Could not load the default permission modes."
    case .catalogue: "Could not load the models. The engine may be busy."
    case .editorPane: "Could not load this file. The engine may be busy."
    }
  }
}

/// What the engine client remembers of one load: which run is the newest, and why the
/// last settled run failed.
struct BackgroundLoadRecord {
  var generation: UInt64 = 0
  var failure: String?
}

extension EngineClient {
  /// How long a catalogue scan may take. A request timeout closes the connection, so a
  /// scan on a busy host gets more room than an ordinary request.
  static let catalogueTimeoutSeconds = 30.0

  public var harnessesLoading: Bool { harnessLoadCount > 0 }

  public func agentCatalogueLoading(_ harness: String) -> Bool {
    agentCatalogueLoadCounts[harness, default: 0] > 0
  }

  /// Why the last attempt to load `load` failed, or nil when it has not failed since
  /// its last success. Views show it inline with a retry.
  public func loadFailure(_ load: BackgroundLoad) -> String? {
    backgroundLoads[load]?.failure
  }

  /// True when there is no harness list to show and the last attempt to load it failed,
  /// for example because the engine was too busy to answer in time.
  public var harnessListStalled: Bool {
    harnesses.isEmpty && !harnessesLoading && loadFailure(.harnesses) != nil
  }

  /// Runs a background load and records its outcome. Unlike `perform`, a failure is
  /// kept per load for the screen that asked, and never raises `problem`. Only the newest
  /// run of a load records an outcome, so a late failure of a superseded run cannot
  /// overwrite what a newer run found.
  @discardableResult
  func performBackground(
    _ load: BackgroundLoad, _ action: @escaping @MainActor () async throws -> Void
  ) -> Task<Void, Never> {
    let generation = backgroundLoads[load, default: BackgroundLoadRecord()].generation &+ 1
    backgroundLoads[load, default: BackgroundLoadRecord()].generation = generation
    return Task {
      do {
        try await action()
        settle(load, generation: generation, failure: nil)
      } catch is CancellationError {
      } catch {
        settle(load, generation: generation, failure: error.localizedDescription)
      }
    }
  }

  private func settle(_ load: BackgroundLoad, generation: UInt64, failure: String?) {
    if backgroundLoads[load]?.generation == generation {
      backgroundLoads[load]?.failure = failure
    }
  }
}
