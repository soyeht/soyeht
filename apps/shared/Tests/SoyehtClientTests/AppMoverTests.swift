#if os(macOS)
  import Foundation
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
      let files = FileManager.default
      let bundle = folder.appending(path: name + ".app", directoryHint: .isDirectory)
      if files.fileExists(atPath: bundle.path) { return bundle }
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
      return bundle
    }

    func stamp(of app: URL) throws -> String {
      String(decoding: try Data(contentsOf: app.appending(path: "Contents/stamp")), as: UTF8.self)
    }

    func mover(_ folders: [URL], trash: (any AppTrashing)? = nil) -> AppMover {
      AppMover(
        bundleId: testBundleId, appName: testAppName, folders: folders, trash: trash ?? self.trash)
    }

    /// An image holding the app, an Applications folder holding an older copy of it, and the
    /// plan to replace that copy.
    func replacement(trash: (any AppTrashing)? = nil) throws -> Replacement {
      let source = try app(in: try folder("image"))
      let applications = try folder("Applications")
      let existing = try app(in: applications, stamp: "old")
      let mover = mover([applications], trash: trash)
      return Replacement(
        source: source, applications: applications, existing: existing, mover: mover,
        plan: try mover.plan(source: source, place: .diskImage))
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
    let source = try box.app(in: try box.folder("image"))
    let system = try box.folder("Applications")
    let own = try box.folder("Home/Applications")
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
    let source = try box.app(in: try box.folder("image"))
    let system = try box.folder("Applications")
    try box.makeReadOnly(system)
    let home = try box.folder("Home")
    let own = home.appending(path: "Applications", directoryHint: .isDirectory)
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
  func anOldCopyInAReadOnlySystemFolderIsNotReplacedThereAndNothingIsWritable() async throws {
    let box = try MoveSandbox()
    let source = try box.app(in: try box.folder("image"))
    let system = try box.folder("Applications")
    let existing = try box.app(in: system, stamp: "old")
    try box.makeReadOnly(system)
    let readOnlyHome = try box.folder("Home")
    try box.makeReadOnly(readOnlyHome)
    let mover = box.mover([system, readOnlyHome.appending(path: "Applications")])
    #expect(throws: AppMoveFailure.noWritableFolder) {
      try mover.plan(source: source, place: .diskImage)
    }
    #expect(try box.stamp(of: existing) == "old")
  }

  @Test func aTrashThatRefusesLeavesTheOldAppAndNoStagingCopy() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement(trash: FailingTrash()).parts
    let applications = existing.deletingLastPathComponent()
    await #expect(throws: AppMoveFailure.replaceFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(try box.stamp(of: existing) == "old")
    #expect(
      try FileManager.default.contentsOfDirectory(atPath: applications.path) == [
        "\(testAppName).app"
      ])
  }

  @Test(.enabled(if: notRoot))
  func aCopyThatStopsHalfWayLeavesTheOldAppAndNoStagingCopy() async throws {
    let box = try MoveSandbox()
    let (existing, mover, plan) = try box.replacement().parts
    let (source, applications) = (plan.source, existing.deletingLastPathComponent())
    // A file that cannot be read stops the copy half way.
    try FileManager.default.setAttributes(
      [.posixPermissions: 0], ofItemAtPath: source.appending(path: "Contents/stamp").path)
    await #expect(throws: AppMoveFailure.copyFailed) {
      try await mover.move(plan, place: .diskImage, confirmedReplace: true)
    }
    #expect(try box.stamp(of: existing) == "old")
    #expect(box.trash.trashed.isEmpty)
    #expect(
      try FileManager.default.contentsOfDirectory(atPath: applications.path) == [
        "\(testAppName).app"
      ])
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
