#if os(macOS)
  import AppKit
  import Foundation
  import Observation
  import SoyehtProtocol

  /// Opens the app at `app` as a new process, so the copy that is being left can quit.
  public protocol AppRelaunching: Sendable {
    func relaunch(_ app: URL) async throws
  }

  public struct WorkspaceRelauncher: AppRelaunching {
    public init() {}

    public func relaunch(_ app: URL) async throws {
      let configuration = NSWorkspace.OpenConfiguration()
      // The copy being left has the same bundle identifier and is still running.
      configuration.createsNewApplicationInstance = true
      _ = try await NSWorkspace.shared.openApplication(at: app, configuration: configuration)
    }
  }

  public struct WorkspaceRunningCheck: AppRunningCheck {
    public init() {}

    public func isRunning(bundleId: String, at app: URL) -> Bool {
      guard let target = AppMover.fileID(of: app) else { return false }
      return NSRunningApplication.runningApplications(withBundleIdentifier: bundleId).contains {
        $0.bundleURL.flatMap { AppMover.fileID(of: $0) } == target
      }
    }
  }

  /// The released app outside a stable place, and the person's way to fix it (ADR 0120, "Where
  /// the app may run"). It offers once to move the app to Applications; a no, or a move that does
  /// not happen, leaves the engine unregistered (`EngineServiceSetup` registers only from a stable
  /// place) and keeps the move one click away: the menu item and the setup row.
  @MainActor @Observable
  public final class AppRelocation {
    public enum Phase: Equatable, Sendable {
      /// The app runs from a stable place, or this profile is not held to the rule.
      case notNeeded
      /// Ask the person, once.
      case offered
      /// An app of this profile is already at the destination; replacing it needs a yes.
      case confirmingReplace(AppMovePlan)
      case moving
      /// The person said no, or did not confirm a replacement.
      case declined
      case failed(AppMoveFailure)
      /// The app was moved but the new copy did not open.
      case relaunchFailed(URL)
      /// Moved, and the new copy is opening: this one is about to quit.
      case moved
    }

    public static let declinedKey = "soyeht.appPlacement.declined"

    public private(set) var phase: Phase
    /// The person closed the message about a move that left the app where it is.
    private var reportClosed = false
    @ObservationIgnored private let app: URL
    @ObservationIgnored private let place: AppPlacement.Place
    @ObservationIgnored private let mover: AppMover
    /// A released copy that starts an engine, wherever it runs.
    @ObservationIgnored private let isReleasedApp: Bool
    @ObservationIgnored private let relauncher: any AppRelaunching
    @ObservationIgnored private let preferences: any ClientPreferenceStore
    @ObservationIgnored private let quit: @MainActor () -> Void

    public convenience init() {
      let bundle = Bundle.main
      let profile = ActiveProfile.values
      let home = FileManager.default.homeDirectoryForCurrentUser
      self.init(
        app: bundle.bundleURL, profile: profile, home: home,
        carriesPrograms: BundledEngineService.command(in: bundle) != nil,
        mover: AppMover(
          bundleId: profile.appBundleId, appName: profile.macAppName,
          folders: AppPlacement.installFolders(home: home)),
        relauncher: WorkspaceRelauncher(), preferences: UserDefaultsClientPreferenceStore(),
        quit: { NSApplication.shared.terminate(nil) })
    }

    /// `carriesPrograms` is whether the bundle holds the command line: a copy without it starts
    /// no engine, so where it runs does not matter.
    public init(
      app: URL, profile: ProfileValues, home: URL, carriesPrograms: Bool, mover: AppMover,
      relauncher: any AppRelaunching, preferences: any ClientPreferenceStore,
      quit: @escaping @MainActor () -> Void
    ) {
      self.app = app
      self.place = AppPlacement.place(of: app, home: home)
      self.mover = mover
      self.relauncher = relauncher
      self.preferences = preferences
      self.quit = quit
      isReleasedApp = carriesPrograms && AppPlacement.appliesTo(profile)
      if !isReleasedApp || place == .stable {
        phase = .notNeeded
      } else {
        phase = preferences.bool(forKey: Self.declinedKey) ? .declined : .offered
      }
    }

    /// The question of the first launch is on screen.
    public var asksToMove: Bool { phase == .offered }

    public var asksToReplace: Bool {
      if case .confirmingReplace = phase { true } else { false }
    }

    /// A message about a move that did not end in a new copy opening, until the person closes it.
    public var report: String? {
      guard !reportClosed else { return nil }
      return switch phase {
      case .failed(let failure): Self.describe(failure)
      case .relaunchFailed: ClientPolicy.appMoveRelaunchFailed
      default: nil
      }
    }

    public var hasReport: Bool { report != nil }

    public var reportTitle: String {
      if case .relaunchFailed = phase {
        ClientPolicy.appMoveRelaunchFailedTitle
      } else {
        ClientPolicy.appMoveFailedTitle
      }
    }

    /// The failure can be followed by a new try; a copy that was moved cannot.
    public var offersRetry: Bool {
      if case .failed = phase { true } else { false }
    }

    /// The person can start a move now: the menu item and the setup row show.
    public var canMove: Bool {
      switch phase {
      case .offered, .declined, .failed: true
      case .notNeeded, .confirmingReplace, .moving, .relaunchFailed, .moved: false
      }
    }

    /// The short explanation of why the engine is off, for the setup row.
    public var explanation: String? {
      switch phase {
      case .notNeeded, .moving, .moved: nil
      case .relaunchFailed: ClientPolicy.appMoveRelaunchFailed
      default: ClientPolicy.appMoveExplanation
      }
    }

    public func decline() {
      guard phase == .offered else { return }
      preferences.set(true, forKey: Self.declinedKey)
      phase = .declined
    }

    /// At launch: removes the hidden copies that a move cut short (a crash, a power cut) left in
    /// the Applications folders.
    public func removeLeftovers() async {
      guard isReleasedApp else { return }
      let mover = mover
      await Task.detached(priority: .utility) { mover.removeLeftovers() }.value
    }

    /// The person agreed to the move, from the question, the menu or the setup row.
    public func move() async {
      guard canMove else { return }
      do {
        let plan = try mover.plan(source: app, place: place)
        if plan.replacesExisting {
          phase = .confirmingReplace(plan)
        } else {
          await run(plan, confirmedReplace: false)
        }
      } catch {
        fail(error)
      }
    }

    public func confirmReplace() async {
      guard case .confirmingReplace(let plan) = phase else { return }
      await run(plan, confirmedReplace: true)
    }

    public func cancelReplace() {
      guard case .confirmingReplace = phase else { return }
      preferences.set(true, forKey: Self.declinedKey)
      phase = .declined
    }

    /// The person closed the message. After a failure the engine stays off and the move stays
    /// available.
    public func acknowledge() {
      reportClosed = true
      if case .failed = phase { phase = .declined }
    }

    private func run(_ plan: AppMovePlan, confirmedReplace: Bool) async {
      phase = .moving
      let destination: URL
      do {
        destination = try await mover.move(plan, place: place, confirmedReplace: confirmedReplace)
      } catch {
        fail(error)
        return
      }
      phase = .moved
      do {
        try await relauncher.relaunch(destination)
      } catch {
        clientLogger.error("app move: relaunch failed")
        reportClosed = false
        phase = .relaunchFailed(destination)
        return
      }
      quit()
    }

    private func fail(_ error: any Error) {
      let failure = error as? AppMoveFailure ?? .copyFailed
      clientLogger.error("app move: refused \(String(describing: failure), privacy: .public)")
      reportClosed = false
      phase = .failed(failure)
    }

    static func describe(_ failure: AppMoveFailure) -> String {
      switch failure {
      case .notThisApp: ClientPolicy.appMoveNotThisApp
      case .destinationIsAnotherApp: ClientPolicy.appMoveAnotherApp
      case .alreadyThere: ClientPolicy.appMoveAlreadyThere
      case .existingCopyLocked: ClientPolicy.appMoveCopyLocked
      case .destinationRunning: ClientPolicy.appMoveRunning
      case .noWritableFolder: ClientPolicy.appMoveNoWritableFolder
      case .replaceNotConfirmed, .planChanged: ClientPolicy.appMoveChanged
      case .copyFailed, .replaceFailed: ClientPolicy.appMoveNotFinished
      case .oldAppInTrash: ClientPolicy.appMoveOldInTrash
      }
    }
  }
#endif
