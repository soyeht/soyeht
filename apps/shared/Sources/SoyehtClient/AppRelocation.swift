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
    @ObservationIgnored private let relauncher: any AppRelaunching
    @ObservationIgnored private let preferences: any ClientPreferenceStore
    @ObservationIgnored private let quit: @MainActor () -> Void

    public convenience init() {
      let bundle = Bundle.main
      let profile = ActiveProfile.values
      let home = FileManager.default.homeDirectoryForCurrentUser
      self.init(
        app: bundle.bundleURL, profile: profile, home: home,
        carriesPrograms: bundle.url(forAuxiliaryExecutable: BundledEngineService.commandName)
          != nil,
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
      if !carriesPrograms || !AppPlacement.appliesTo(profile) || place == .stable {
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
      case .relaunchFailed: Words.relaunchFailed
      default: nil
      }
    }

    public var hasReport: Bool { report != nil }

    public var reportTitle: String {
      if case .relaunchFailed = phase { Words.relaunchFailedTitle } else { Words.failureTitle }
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
      case .relaunchFailed: Words.relaunchFailed
      default: Words.explanation
      }
    }

    public func decline() {
      guard phase == .offered else { return }
      preferences.set(true, forKey: Self.declinedKey)
      phase = .declined
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
      case .notThisApp: Words.notThisApp
      case .destinationIsAnotherApp: Words.anotherApp
      case .noWritableFolder: Words.noWritableFolder
      case .replaceNotConfirmed, .planChanged: Words.changed
      case .copyFailed, .replaceFailed: Words.notFinished
      }
    }

    /// The words of the offer, the confirmation, the failure and the setup row. Only the Mac has
    /// this step, so they are not in `x-clientPolicy`.
    public enum Words {
      public static let question = "Move Soyeht to Applications?"
      public static let move = "Move to Applications"
      public static let moveMenu = "Move to Applications…"
      public static let notNow = "Not now"
      public static let cancel = "Cancel"
      public static let explanation =
        "The engine is off. It needs Soyeht in your Applications folder so macOS remembers its permissions."
      public static let summary =
        "Soyeht's engine needs the app in your Applications folder so macOS remembers the permissions you give it. Soyeht copies itself there and opens again. If it runs from a disk image, you can eject the image afterwards."
      public static let replaceQuestion = "Replace the app in Applications?"
      public static let replaceSummary =
        "Soyeht is already in Applications. The old copy goes to the Trash."
      public static let replace = "Replace"
      public static let failureTitle = "Soyeht could not move itself"
      public static let failureLater =
        "The engine stays off until Soyeht is in Applications. You can drag it there yourself, or choose Move to Applications again."
      public static let tryAgain = "Try again"
      public static let ok = "OK"
      public static let relaunchFailedTitle = "Soyeht is now in Applications"
      public static let relaunchFailed =
        "Soyeht was moved to Applications but could not open there. Open it from Applications."
      static let notThisApp = "This copy is not the app Soyeht expected, so nothing was changed."
      static let anotherApp =
        "A different app with the same name is in Applications. Soyeht did not touch it."
      static let noWritableFolder =
        "Neither the Applications folder nor your own Applications folder can be written to."
      static let changed = "Applications changed while Soyeht was moving. Nothing was replaced."
      static let notFinished = "The copy did not finish. The app that was there is unchanged."
    }
  }
#endif
