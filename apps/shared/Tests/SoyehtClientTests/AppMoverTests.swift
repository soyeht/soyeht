#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing
  import os

  @testable import SoyehtClient

  let testBundleId = "com.example.next.mac"
  let testAppName = "Soyeht Test"

  /// A temp folder that holds fake apps, fake Applications folders and a fake Trash.
  final class MoveSandbox: Sendable {
    let root: URL
    let trash: FakeTrash

    init() throws {
      root = FileManager.default.temporaryDirectory
        .appending(path: "soyeht-move-\(UUID().uuidString)", directoryHint: .isDirectory)
      try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
      trash = try FakeTrash(folder: root.appending(path: "Trash"))
    }

    deinit {
      let files = FileManager.default
      guard let all = files.enumerator(at: root, includingPropertiesForKeys: nil) else { return }
      for case let item as URL in all {
        try? files.setAttributes([.posixPermissions: 0o755], ofItemAtPath: item.path)
      }
      try? files.removeItem(at: root)
    }

    func folder(_ name: String) throws -> URL {
      let url = root.appending(path: name, directoryHint: .isDirectory)
      try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
      return url
    }

    /// An app bundle with an executable, a resource and a framework-style link, marked with
    /// `stamp` so a test can tell one copy from another.
    @discardableResult
    func app(
      in folder: URL, named name: String = testAppName, id: String = testBundleId,
      stamp: String = "new"
    ) throws -> URL {
      let bundle = folder.appending(path: name + ".app", directoryHint: .isDirectory)
      if FileManager.default.fileExists(atPath: bundle.path) { return bundle }
      try Self.writeApp(at: bundle, id: id, stamp: stamp)
      return bundle
    }

    static func writeApp(at bundle: URL, id: String, stamp: String) throws {
      let files = FileManager.default
      let macOS = bundle.appending(path: "Contents/MacOS")
      try files.createDirectory(at: macOS, withIntermediateDirectories: true)
      let info: [String: Any] = ["CFBundleIdentifier": id, "CFBundleExecutable": "app"]
      try PropertyListSerialization.data(fromPropertyList: info, format: .xml, options: 0)
        .write(to: bundle.appending(path: "Contents/Info.plist"))
      let executable = macOS.appending(path: "app")
      try Data("#!/bin/sh\n".utf8).write(to: executable)
      try files.setAttributes([.posixPermissions: 0o755], ofItemAtPath: executable.path)
      try Data(stamp.utf8).write(to: bundle.appending(path: "Contents/stamp"))
      try files.createSymbolicLink(
        atPath: bundle.appending(path: "Contents/current").path, withDestinationPath: "stamp")
    }

    func stamp(of app: URL) throws -> String {
      String(decoding: try Data(contentsOf: app.appending(path: "Contents/stamp")), as: UTF8.self)
    }

    func mover(
      _ folders: [URL], trash: (any AppTrashing)? = nil,
      running: any AppRunningCheck = NothingRuns(),
      signature: any AppCodeValidating = ValidSignature()
    ) -> AppMover {
      AppMover(
        bundleId: testBundleId, appName: testAppName, folders: folders, trash: trash ?? self.trash,
        running: running, signature: signature)
    }

    /// An image holding the app, an Applications folder holding an older copy of it, and the
    /// plan to replace that copy.
    func replacement(
      trash: (any AppTrashing)? = nil, running: any AppRunningCheck = NothingRuns(),
      signature: any AppCodeValidating = ValidSignature()
    ) throws -> Replacement {
      let source = try app(in: try folder("image"))
      let applications = try folder("Applications")
      let existing = try app(in: applications, stamp: "old")
      let mover = mover([applications], trash: trash, running: running, signature: signature)
      return Replacement(
        source: source, applications: applications, existing: existing, mover: mover,
        plan: try mover.plan(source: source, place: .diskImage))
    }

    /// An image holding the app, the system Applications folder and the person's own, empty.
    func imageAndTwoFolders() throws -> (source: URL, system: URL, own: URL) {
      (
        try app(in: try folder("image")), try folder("Applications"),
        try folder("Home/Applications")
      )
    }

    /// `folder` holds only the app named for the profile, marked `stamp`, and nothing was trashed.
    func expectOnlyThe(app: URL, stamp expected: String) throws {
      #expect(try stamp(of: app) == expected)
      #expect(trash.trashed.isEmpty)
      let folder = app.deletingLastPathComponent()
      #expect(
        try FileManager.default.contentsOfDirectory(atPath: folder.path) == [
          "\(testAppName).app"
        ])
    }

    func makeReadOnly(_ url: URL) throws {
      try FileManager.default.setAttributes([.posixPermissions: 0o555], ofItemAtPath: url.path)
    }
  }

  struct Replacement {
    let source: URL
    let applications: URL
    let existing: URL
    let mover: AppMover
    let plan: AppMovePlan

    var parts: (URL, AppMover, AppMovePlan) { (existing, mover, plan) }
  }

  /// "Trash" is a folder of the sandbox, and it remembers what it was given.
  final class FakeTrash: AppTrashing {
    private let folder: URL
    private let items = OSAllocatedUnfairLock<[URL]>(initialState: [])

    init(folder: URL) throws {
      self.folder = folder
      try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    }

    var trashed: [URL] { items.withLock { $0 } }

    func trash(_ url: URL) throws -> URL {
      let target = folder.appending(path: UUID().uuidString + "-" + url.lastPathComponent)
      try FileManager.default.moveItem(at: url, to: target)
      items.withLock { $0.append(url) }
      return target
    }
  }

  /// The fake apps of these tests carry no signature: this one takes every copy as whole.
  struct ValidSignature: AppCodeValidating {
    func isValid(_ app: URL) -> Bool { true }
  }

  /// A copy whose seal is broken.
  struct BrokenSignature: AppCodeValidating {
    func isValid(_ app: URL) -> Bool { false }
  }

  /// Remembers what it was asked about, and what the destination held at that moment.
  final class RecordsTheCheck: AppCodeValidating {
    private let seen = OSAllocatedUnfairLock<[(URL, String?)]>(initialState: [])
    let destination: URL
    init(destination: URL) { self.destination = destination }

    var asked: [(URL, String?)] { seen.withLock { $0 } }

    func isValid(_ app: URL) -> Bool {
      let held = try? String(
        contentsOf: destination.appending(path: "Contents/stamp"), encoding: .utf8)
      seen.withLock { $0.append((app, held)) }
      return true
    }
  }

  /// A Trash that takes the item and does not say where it put it.
  final class TrashesWithoutSayingWhere: AppTrashing {
    let inner: FakeTrash
    init(_ inner: FakeTrash) { self.inner = inner }

    func trash(_ url: URL) throws -> URL {
      _ = try inner.trash(url)
      throw AppMoveFailure.oldAppInTrash
    }
  }

  struct NothingRuns: AppRunningCheck {
    func isRunning(bundleId: String, at app: URL) -> Bool { false }
  }

  /// Answers like a copy that is not running until the `swapAt`th question, which it uses to put
  /// another bundle in the place of the app, as another process could while the copy is made.
  final class SwapsTheDestination: AppRunningCheck {
    private let asked = OSAllocatedUnfairLock(initialState: 0)
    let swapAt: Int
    let swap: @Sendable (URL) throws -> Void
    init(swapAt: Int, swap: @escaping @Sendable (URL) throws -> Void) {
      self.swapAt = swapAt
      self.swap = swap
    }

    func isRunning(bundleId: String, at app: URL) -> Bool {
      let count = asked.withLock { value -> Int in
        value += 1
        return value
      }
      if count == swapAt { try? swap(app) }
      return false
    }
  }

  /// Not running when the move is planned and re-planned, running by the time the copy is done.
  final class RunsFromTheThirdQuestion: AppRunningCheck {
    private let asked = OSAllocatedUnfairLock(initialState: 0)
    func isRunning(bundleId: String, at app: URL) -> Bool {
      asked.withLock { value -> Bool in
        value += 1
        return value >= 3
      }
    }
  }

  struct Running: AppRunningCheck {
    func isRunning(bundleId: String, at app: URL) -> Bool { true }
  }

  /// A Trash that takes the old app and then loses the new copy's staging folder, so the rename
  /// fails; with `blocksRestore` it also puts a folder where the old app was, so putting it back
  /// fails too.
  final class LosesTheStaging: AppTrashing {
    let inner: FakeTrash
    let blocksRestore: Bool
    init(_ inner: FakeTrash, blocksRestore: Bool) {
      self.inner = inner
      self.blocksRestore = blocksRestore
    }

    func trash(_ url: URL) throws -> URL {
      let trashed = try inner.trash(url)
      let folder = url.deletingLastPathComponent()
      for name in try FileManager.default.contentsOfDirectory(atPath: folder.path)
      where name.contains(".moving.") {
        try FileManager.default.removeItem(at: folder.appending(path: name))
      }
      if blocksRestore {
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
      }
      return trashed
    }
  }

  private struct FailingTrash: AppTrashing {
    func trash(_ url: URL) throws -> URL { throw AppMoveFailure.replaceFailed }
  }

  private let notRoot = getuid() != 0

  @Test func aDiskImageCopyIsCopiedAndTheImageIsLeftAsItWas() async throws {
    let box = try MoveSandbox()
    let image = try box.folder("image")
    let source = try box.app(in: image)
    try box.makeReadOnly(source)
    let applications = try box.folder("Applications")

    let mover = box.mover([applications])
    let plan = try mover.plan(source: source, place: .diskImage)
    #expect(
      plan.destination
        == applications.appending(path: "\(testAppName).app", directoryHint: .isDirectory))
    #expect(!plan.replacesExisting)
    #expect(plan.keepsSource)
    let moved = try await mover.move(plan, place: .diskImage, confirmedReplace: false)

    #expect(moved == plan.destination)
    #expect(try box.stamp(of: moved) == "new")
    #expect(
      FileManager.default.isExecutableFile(atPath: moved.appending(path: "Contents/MacOS/app").path)
    )
    // A link inside the bundle stays a link.
    #expect(
      try FileManager.default.destinationOfSymbolicLink(
        atPath: moved.appending(path: "Contents/current").path) == "stamp")
    #expect(FileManager.default.fileExists(atPath: source.path), "the image is not touched")
    #expect(box.trash.trashed.isEmpty)
    let leftovers = try FileManager.default.contentsOfDirectory(atPath: applications.path)
    #expect(leftovers == ["\(testAppName).app"], "no staging copy is left behind")
  }

  @Test func anAppFromAnotherFolderIsMovedSoTheOriginalGoesToTheTrash() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("Downloads"))
    let applications = try box.folder("Applications")
    let mover = box.mover([applications])
    let plan = try mover.plan(source: source, place: .elsewhere)
    #expect(!plan.keepsSource)
    try await mover.move(plan, place: .elsewhere, confirmedReplace: false)
    #expect(box.trash.trashed == [source])
    #expect(!FileManager.default.fileExists(atPath: source.path))
    #expect(
      try box.stamp(
        of: applications.appending(path: "\(testAppName).app", directoryHint: .isDirectory))
        == "new")
  }

  @Test func aCopyAlreadyThereIsReplacedOnlyAfterAYesAndGoesToTheTrash() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement().parts
    #expect(plan.replacesExisting)
    await #expect(throws: AppMoveFailure.replaceNotConfirmed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: false)
    }
    #expect(try box.stamp(of: existing) == "old")
    #expect(box.trash.trashed.isEmpty)

    try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    #expect(try box.stamp(of: existing) == "new")
    #expect(box.trash.trashed == [existing])
    let kept = try FileManager.default.contentsOfDirectory(
      at: box.root.appending(path: "Trash"), includingPropertiesForKeys: nil)
    #expect(
      try kept.map { try box.stamp(of: $0) } == ["old"], "the old app is in the Trash, not deleted")
  }

  @Test func anotherAppWithTheNameIsNeverReplacedNorBesideLeftACopy() async throws {
    let box = try MoveSandbox()
    let (source, system, own) = try box.imageAndTwoFolders()
    let stranger = try box.app(in: system, id: "com.example.stranger", stamp: "stranger")
    let mover = box.mover([system, own])
    #expect(throws: AppMoveFailure.destinationIsAnotherApp) {
      try mover.plan(source: source, place: .diskImage)
    }
    #expect(try box.stamp(of: stranger) == "stranger")
    #expect(box.trash.trashed.isEmpty)
    #expect(try FileManager.default.contentsOfDirectory(atPath: own.path).isEmpty)
  }

  @Test func aBundleOfAnotherIdentifierIsNotMovedAndNeitherIsAFolderThatIsNotAnApp() async throws {
    let box = try MoveSandbox()
    let applications = try box.folder("Applications")
    let mover = box.mover([applications])
    let other = try box.app(in: try box.folder("image"), id: "com.example.other")
    #expect(throws: AppMoveFailure.notThisApp) { try mover.plan(source: other, place: .diskImage) }
    let plain = try box.folder("image/NotAnApp")
    #expect(throws: AppMoveFailure.notThisApp) { try mover.plan(source: plain, place: .diskImage) }
    let renamed = try box.app(in: try box.folder("moved"), named: "Soyeht Test 2")
    let renamedPlan = try mover.plan(source: renamed, place: .elsewhere)
    #expect(
      renamedPlan.destination.lastPathComponent == "\(testAppName).app",
      "the destination is named by the profile, not by the file that was opened")
    #expect(try FileManager.default.contentsOfDirectory(atPath: applications.path).isEmpty)
  }

  @Test(.enabled(if: notRoot))
  func aReadOnlySystemFolderFallsBackToTheUsersAndCreatesIt() async throws {
    let box = try MoveSandbox()
    let (source, system, own) = try box.imageAndTwoFolders()
    try box.makeReadOnly(system)
    try FileManager.default.removeItem(at: own)
    let mover = box.mover([system, own])

    let plan = try mover.plan(source: source, place: .diskImage)
    #expect(
      plan.destination == own.appending(path: "\(testAppName).app", directoryHint: .isDirectory))
    #expect(plan.createsFolder)
    try await mover.move(plan, place: .diskImage, confirmedReplace: false)
    #expect(try box.stamp(of: plan.destination) == "new")
    #expect(try FileManager.default.contentsOfDirectory(atPath: system.path).isEmpty)
  }

  @Test(.enabled(if: notRoot))
  func whenNeitherFolderCanBeWrittenNothingIsMoved() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("image"))
    let system = try box.folder("Applications")
    try box.makeReadOnly(system)
    let readOnlyHome = try box.folder("Home")
    try box.makeReadOnly(readOnlyHome)
    let mover = box.mover([system, readOnlyHome.appending(path: "Applications")])
    #expect(throws: AppMoveFailure.noWritableFolder) {
      try mover.plan(source: source, place: .diskImage)
    }
    #expect(try FileManager.default.contentsOfDirectory(atPath: system.path).isEmpty)
  }

  @Test func aTrashThatRefusesLeavesTheOldAppAndNoStagingCopy() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement(trash: FailingTrash()).parts
    await #expect(throws: AppMoveFailure.replaceFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    try box.expectOnlyThe(app: existing, stamp: "old")
  }

  @Test(.enabled(if: notRoot))
  func aCopyThatStopsHalfWayLeavesTheOldAppAndNoStagingCopy() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement().parts
    let source = plan.source
    // A file that cannot be read stops the copy half way.
    try FileManager.default.setAttributes(
      [.posixPermissions: 0], ofItemAtPath: source.appending(path: "Contents/stamp").path)
    await #expect(throws: AppMoveFailure.copyFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    try box.expectOnlyThe(app: existing, stamp: "old")
  }

  @Test func aCopyWhoseSignatureIsBrokenIsNeverPutInPlace() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement(signature: BrokenSignature()).parts
    await #expect(throws: AppMoveFailure.copyFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    try box.expectOnlyThe(app: existing, stamp: "old")
  }

  @Test func theSignatureOfTheStagedCopyIsCheckedWhileTheOldAppIsStillInPlace() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("image"))
    let applications = try box.folder("Applications")
    let existing = try box.app(in: applications, stamp: "old")
    let check = RecordsTheCheck(destination: existing)
    let mover = box.mover([applications], signature: check)
    let plan = try mover.plan(source: source, place: .diskImage)
    try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    let asked = check.asked
    #expect(asked.count == 1)
    #expect(asked[0].0.deletingLastPathComponent() == applications)
    #expect(asked[0].0.lastPathComponent.hasPrefix(".\(testAppName).moving."))
    #expect(asked[0].1 == "old", "nothing was trashed or replaced yet")
    #expect(try box.stamp(of: existing) == "new")
  }

  @Test func aTrashThatTookTheOldAppWithoutSayingWhereIsReportedAsBeingInTheTrash() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement(
      trash: TrashesWithoutSayingWhere(box.trash)
    ).parts
    await #expect(throws: AppMoveFailure.oldAppInTrash) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(box.trash.trashed == [existing])
    #expect(!FileManager.default.fileExists(atPath: existing.path))
    let folder = existing.deletingLastPathComponent()
    #expect(try FileManager.default.contentsOfDirectory(atPath: folder.path).isEmpty)
  }

  @Test func theSystemTrashSaysWhereAnItemWentOrThatItDidNot() throws {
    let url = URL(fileURLWithPath: "/tmp/somewhere-in-the-trash")
    #expect(try SystemTrash.located(url as NSURL) == url)
    #expect(throws: AppMoveFailure.oldAppInTrash) { try SystemTrash.located(nil) }
  }

  @Test func aDestinationThatChangedSinceThePlanIsNotTouched() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("image"))
    let applications = try box.folder("Applications")
    let mover = box.mover([applications])
    let plan = try mover.plan(source: source, place: .diskImage)
    let appeared = try box.app(in: applications, stamp: "appeared")
    await #expect(throws: AppMoveFailure.planChanged) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(try box.stamp(of: appeared) == "appeared")
    #expect(box.trash.trashed.isEmpty)
  }
#endif

#if os(macOS)
  private func hasQuarantine(_ url: URL) -> Bool {
    url.withUnsafeFileSystemRepresentation { path in
      path.map { getxattr($0, "com.apple.quarantine", nil, 0, 0, XATTR_NOFOLLOW) >= 0 } ?? false
    }
  }

  private func quarantine(_ url: URL) {
    let value = Array("0083;5f000000;Safari;".utf8)
    _ = url.withUnsafeFileSystemRepresentation { path in
      path.map { setxattr($0, "com.apple.quarantine", value, value.count, 0, XATTR_NOFOLLOW) }
    }
  }

  @Test func theCopyIsNotQuarantinedAgainAndTheSourceKeepsItsMark() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("Downloads"))
    let inner = source.appending(path: "Contents/stamp")
    quarantine(source)
    quarantine(inner)
    #expect(hasQuarantine(source) && hasQuarantine(inner))
    let mover = box.mover([try box.folder("Applications")])
    let plan = try mover.plan(source: source, place: .diskImage)
    let moved = try await mover.move(plan, place: .diskImage, confirmedReplace: false)
    #expect(!hasQuarantine(moved) && !hasQuarantine(moved.appending(path: "Contents/stamp")))
    #expect(hasQuarantine(source), "a disk image or translocated source is not touched")
  }

  @Test func aLinkToTheAppIsCopiedAsTheAppNotAsALinkAndNothingIsTrashed() async throws {
    let box = try MoveSandbox()
    let real = try box.app(in: try box.folder("Elsewhere"), stamp: "real")
    let link = try box.folder("Downloads").appending(path: "\(testAppName).app")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real)
    let applications = try box.folder("Applications")
    let mover = box.mover([applications])
    let plan = try mover.plan(source: link, place: .elsewhere)
    #expect(plan.keepsSource, "neither the link nor the app it points to is trashed")
    let moved = try await mover.move(plan, place: .elsewhere, confirmedReplace: false)
    let attributes = try FileManager.default.attributesOfItem(atPath: moved.path)
    #expect(attributes[.type] as? FileAttributeType == .typeDirectory, "a bundle, not a link")
    #expect(try box.stamp(of: moved) == "real")
    #expect(try box.stamp(of: real) == "real")
    #expect(FileManager.default.fileExists(atPath: link.path))
    #expect(box.trash.trashed.isEmpty)
  }

  @Test func aSymlinkToTheCopyInApplicationsIsTheSameAppAndNothingIsTrashed() async throws {
    let box = try MoveSandbox()
    let applications = try box.folder("Applications")
    let existing = try box.app(in: applications, stamp: "only")
    let link = try box.folder("Downloads").appending(path: "\(testAppName).app")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: existing)
    let mover = box.mover([applications])
    #expect(throws: AppMoveFailure.alreadyThere) { try mover.plan(source: link, place: .elsewhere) }
    #expect(try box.stamp(of: existing) == "only")
    #expect(box.trash.trashed.isEmpty)
  }

  @Test(.enabled(if: notRoot))
  func aCopyInALockedSystemFolderIsNotLeftBesideAnotherInTheUsers() async throws {
    let box = try MoveSandbox()
    let (source, system, own) = try box.imageAndTwoFolders()
    let installed = try box.app(in: system, stamp: "admin")
    try box.makeReadOnly(system)
    let mover = box.mover([system, own])
    #expect(throws: AppMoveFailure.existingCopyLocked) {
      try mover.plan(source: source, place: .diskImage)
    }
    #expect(try box.stamp(of: installed) == "admin")
    #expect(try FileManager.default.contentsOfDirectory(atPath: own.path).isEmpty)
  }

  @Test func aCopyInTheUsersFolderIsTheOneReplacedNotANewOneInTheSystemFolder() async throws {
    let box = try MoveSandbox()
    let (source, system, own) = try box.imageAndTwoFolders()
    let installed = try box.app(in: own, stamp: "mine")
    let mover = box.mover([system, own])
    let plan = try mover.plan(source: source, place: .diskImage)
    #expect(plan.destination == installed && plan.replacesExisting)
    try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    #expect(try box.stamp(of: installed) == "new")
    #expect(try FileManager.default.contentsOfDirectory(atPath: system.path).isEmpty)
  }

  @Test func aCopyThatRunsIsNotReplacedAndIsNotTrashed() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("image"))
    let applications = try box.folder("Applications")
    let running = try box.app(in: applications, stamp: "running")
    let mover = box.mover([applications], running: Running())
    #expect(throws: AppMoveFailure.destinationRunning) {
      try mover.plan(source: source, place: .diskImage)
    }
    #expect(try box.stamp(of: running) == "running")
    #expect(box.trash.trashed.isEmpty)
  }

  @Test func aCopyThatStartsRunningDuringTheCopyIsNotTrashed() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement(running: RunsFromTheThirdQuestion()).parts
    await #expect(throws: AppMoveFailure.destinationRunning) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    try box.expectOnlyThe(app: existing, stamp: "old")
  }

  @Test func somethingElseThatLandsAtTheDestinationDuringTheCopyIsNotTrashed() async throws {
    let box = try MoveSandbox()
    let applications = try box.folder("Applications")
    _ = try box.app(in: try box.folder("image"))
    let existing = try box.app(in: applications, stamp: "old")
    let swapper = SwapsTheDestination(swapAt: 3) { app in
      try FileManager.default.removeItem(at: app)
      try MoveSandbox.writeApp(at: app, id: testBundleId, stamp: "intruder")
    }
    let source = box.root.appending(path: "image/\(testAppName).app")
    let mover = box.mover([applications], running: swapper)
    let plan = try mover.plan(source: source, place: .diskImage)
    await #expect(throws: AppMoveFailure.planChanged) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    try box.expectOnlyThe(app: existing, stamp: "intruder")
  }

  @Test func aFailedRenamePutsTheOldAppBackFromTheTrash() async throws {
    let box = try MoveSandbox()
    let trash = LosesTheStaging(box.trash, blocksRestore: false)
    let (existing, mover, plan) = try box.replacement(trash: trash).parts
    await #expect(throws: AppMoveFailure.replaceFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(try box.stamp(of: existing) == "old", "the old app is back where it was")
  }

  @Test func anOldAppThatCannotBePutBackIsReportedAsBeingInTheTrash() async throws {
    let box = try MoveSandbox()
    let trash = LosesTheStaging(box.trash, blocksRestore: true)
    let (existing, mover, plan) = try box.replacement(trash: trash).parts
    await #expect(throws: AppMoveFailure.oldAppInTrash) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(box.trash.trashed == [existing])
    let kept = try FileManager.default.contentsOfDirectory(
      at: box.root.appending(path: "Trash"), includingPropertiesForKeys: nil)
    #expect(try kept.map { try box.stamp(of: $0) } == ["old"])
    #expect(ClientPolicy.appMoveOldInTrash.contains("Trash"))
  }

  @Test func hiddenCopiesLeftByACutShortMoveAreRemovedAndNothingElseIs() throws {
    let box = try MoveSandbox()
    let applications = try box.folder("Applications")
    let leftover = try box.folder("Applications/.\(testAppName).moving.\(UUID().uuidString)")
    let installed = try box.app(in: applications, stamp: "keep")
    let stranger = try box.folder("Applications/.\(testAppName).moving.notes")
    let missing = applications.deletingLastPathComponent().appending(path: "Home/Applications")
    box.mover([applications, missing]).removeLeftovers()
    #expect(!FileManager.default.fileExists(atPath: leftover.path))
    #expect(FileManager.default.fileExists(atPath: stranger.path))
    #expect(try box.stamp(of: installed) == "keep")
  }
#endif
