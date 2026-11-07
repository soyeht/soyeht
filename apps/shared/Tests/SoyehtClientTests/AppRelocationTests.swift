#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing
  import os

  @testable import SoyehtClient

  private final class RelocationPreferences: ClientPreferenceStore {
    var values: [String: Bool] = [:]
    func bool(forKey key: String) -> Bool { values[key] ?? false }
    func storedBool(forKey key: String) -> Bool? { values[key] }
    func set(_ value: Bool, forKey key: String) { values[key] = value }
    func integer(forKey key: String) -> Int? { nil }
    func set(_ value: Int, forKey key: String) {}
  }

  private final class RecordingRelauncher: AppRelaunching {
    private let opened = OSAllocatedUnfairLock<[URL]>(initialState: [])
    let fails: Bool
    init(fails: Bool = false) { self.fails = fails }
    var apps: [URL] { opened.withLock { $0 } }
    func relaunch(_ app: URL) async throws {
      opened.withLock { $0.append(app) }
      if fails { throw AppMoveFailure.copyFailed }
    }
  }

  /// One launch of the app from `source`, with its Applications folders and its Trash in a
  /// sandbox, and a count of the times it quit.
  @MainActor private struct Launch {
    let box: MoveSandbox
    let relocation: AppRelocation
    let relauncher: RecordingRelauncher
    let preferences: RelocationPreferences
    let quits: OSAllocatedUnfairLock<Int>
    let applications: URL

    var quitCount: Int { quits.withLock { $0 } }

    /// `place` is the folder the running copy sits in, relative to the sandbox. A folder named
    /// `AppTranslocation` makes it a translocated copy, which is how the rule sees one.
    init(
      box: MoveSandbox, place: String = "AppTranslocation/ID/d", profile: ProfileValues = .prod,
      carriesPrograms: Bool = true, relauncherFails: Bool = false,
      preferences: RelocationPreferences = RelocationPreferences(),
      readOnlyApplications: Bool = false
    ) throws {
      let source = try box.app(in: try box.folder(place))
      let applications = try box.folder("Applications")
      if readOnlyApplications { try box.makeReadOnly(applications) }
      let home = try box.folder("Home")
      let relauncher = RecordingRelauncher(fails: relauncherFails)
      let quits = OSAllocatedUnfairLock(initialState: 0)
      self.box = box
      self.applications = applications
      self.relauncher = relauncher
      self.preferences = preferences
      self.quits = quits
      relocation = AppRelocation(
        app: source, profile: profile, home: home, carriesPrograms: carriesPrograms,
        mover: box.mover([applications]), relauncher: relauncher, preferences: preferences,
        quit: { quits.withLock { $0 += 1 } })
    }

    var destination: URL {
      applications.appending(path: "\(testAppName).app", directoryHint: .isDirectory)
    }
  }

  @MainActor
  @Test(arguments: [
    ("a development profile", ProfileValues.dev, true, "AppTranslocation/ID/d"),
    ("a copy with no programs", ProfileValues.prod, false, "AppTranslocation/ID/d"),
  ])
  func aCopyThatStartsNoEngineOrIsNotReleasedIsLeftWhereItIs(
    _ why: String, profile: ProfileValues, carriesPrograms: Bool, place: String
  ) throws {
    let launch = try Launch(
      box: MoveSandbox(), place: place, profile: profile, carriesPrograms: carriesPrograms)
    #expect(launch.relocation.phase == .notNeeded)
    #expect(!launch.relocation.asksToMove && !launch.relocation.canMove)
    #expect(launch.relocation.explanation == nil)
  }

  @MainActor @Test func aCopyInApplicationsAsksNothing() throws {
    let box = try MoveSandbox()
    let home = try box.folder("Home")
    let source = try box.app(in: try box.folder("Home/Applications/Tools"))
    let relocation = AppRelocation(
      app: source, profile: .prod, home: home, carriesPrograms: true, mover: box.mover([]),
      relauncher: RecordingRelauncher(), preferences: RelocationPreferences(), quit: {})
    #expect(relocation.phase == .notNeeded)
  }

  @MainActor @Test func theFirstLaunchOutsideApplicationsAsksOnceAndANoIsRemembered() throws {
    let box = try MoveSandbox()
    let first = try Launch(box: box)
    #expect(first.relocation.phase == .offered)
    #expect(first.relocation.asksToMove)

    first.relocation.decline()
    #expect(first.relocation.phase == .declined)
    #expect(!first.relocation.asksToMove)
    #expect(first.relocation.canMove, "the move stays one click away")
    #expect(first.relocation.explanation == AppRelocation.Words.explanation)
    #expect(!FileManager.default.fileExists(atPath: first.destination.path))

    // The next launch does not ask again; the menu and the setup row still offer it.
    let next = try Launch(box: box, preferences: first.preferences)
    #expect(next.relocation.phase == .declined)
    #expect(!next.relocation.asksToMove)
    #expect(next.relocation.canMove)
  }

  @MainActor @Test func agreeingCopiesTheAppOpensTheNewOneAndQuitsThisOne() async throws {
    let launch = try Launch(box: MoveSandbox())
    await launch.relocation.move()
    #expect(launch.relocation.phase == .moved)
    #expect(launch.relauncher.apps == [launch.destination])
    #expect(launch.quitCount == 1)
    #expect(try launch.box.stamp(of: launch.destination) == "new")
  }

  @MainActor @Test func anExistingCopyAsksBeforeItIsReplacedAndNoLeavesEverythingAsItWas()
    async throws
  {
    let launch = try Launch(box: MoveSandbox())
    let existing = try launch.box.app(in: launch.applications, stamp: "old")
    await launch.relocation.move()
    #expect(launch.relocation.asksToReplace)
    #expect(try launch.box.stamp(of: existing) == "old")
    #expect(launch.relauncher.apps.isEmpty && launch.quitCount == 0)

    launch.relocation.cancelReplace()
    #expect(launch.relocation.phase == .declined)
    #expect(try launch.box.stamp(of: existing) == "old")
    #expect(launch.box.trash.trashed.isEmpty)
    #expect(launch.preferences.bool(forKey: AppRelocation.declinedKey))

    await launch.relocation.move()
    await launch.relocation.confirmReplace()
    #expect(try launch.box.stamp(of: existing) == "new")
    #expect(launch.box.trash.trashed == [existing])
    #expect(launch.relauncher.apps == [existing] && launch.quitCount == 1)
  }

  @MainActor @Test(.enabled(if: getuid() != 0))
  func aMoveThatCannotHappenExplainsItQuitsNothingAndCanBeTriedAgain() async throws {
    let launch = try Launch(box: MoveSandbox(), readOnlyApplications: true)
    await launch.relocation.move()
    #expect(launch.relocation.phase == .failed(.noWritableFolder))
    #expect(launch.relocation.report == AppRelocation.Words.noWritableFolder)
    #expect(launch.relocation.offersRetry)
    #expect(launch.relauncher.apps.isEmpty && launch.quitCount == 0)

    launch.relocation.acknowledge()
    #expect(launch.relocation.phase == .declined)
    #expect(launch.relocation.canMove)
    #expect(launch.relocation.explanation == AppRelocation.Words.explanation)

    try FileManager.default.setAttributes(
      [.posixPermissions: 0o755], ofItemAtPath: launch.applications.path)
    await launch.relocation.move()
    #expect(launch.relocation.phase == .moved)
    #expect(launch.quitCount == 1)
  }

  @MainActor @Test func aCopyOfAnotherAppIsNeverMovedWhateverItIsCalled() async throws {
    let box = try MoveSandbox()
    let applications = try box.folder("Applications")
    let other = try box.app(in: try box.folder("AppTranslocation/ID/d"), id: "com.example.other")
    let relauncher = RecordingRelauncher()
    let relocation = AppRelocation(
      app: other, profile: .prod, home: try box.folder("Home"), carriesPrograms: true,
      mover: box.mover([applications]), relauncher: relauncher,
      preferences: RelocationPreferences(), quit: {})
    await relocation.move()
    #expect(relocation.phase == .failed(.notThisApp))
    #expect(relauncher.apps.isEmpty)
    #expect(try FileManager.default.contentsOfDirectory(atPath: applications.path).isEmpty)
  }

  @MainActor @Test func aMovedAppThatCannotOpenKeepsThisCopyRunningAndSaysWhere() async throws {
    let launch = try Launch(box: MoveSandbox(), relauncherFails: true)
    await launch.relocation.move()
    #expect(launch.relocation.phase == .relaunchFailed(launch.destination))
    #expect(launch.relocation.report == AppRelocation.Words.relaunchFailed)
    #expect(!launch.relocation.offersRetry)
    #expect(launch.quitCount == 0)
    #expect(!launch.relocation.canMove, "the app is already there; asking again would replace it")
    #expect(launch.relocation.explanation == AppRelocation.Words.relaunchFailed)
    launch.relocation.acknowledge()
    #expect(launch.relocation.report == nil, "a closed message is not shown again")
    #expect(launch.relocation.phase == .relaunchFailed(launch.destination))
  }

  @MainActor @Test func theQuestionsAreOnlyAnsweredWhileTheyAreAsked() async throws {
    let launch = try Launch(box: MoveSandbox())
    launch.relocation.cancelReplace()
    await launch.relocation.confirmReplace()
    launch.relocation.acknowledge()
    #expect(launch.relocation.phase == .offered)
    launch.relocation.decline()
    launch.relocation.decline()
    #expect(launch.relocation.phase == .declined)
    #expect(launch.relauncher.apps.isEmpty && launch.quitCount == 0)
  }
#endif
