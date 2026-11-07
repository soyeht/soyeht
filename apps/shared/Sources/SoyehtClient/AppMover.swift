#if os(macOS)
  import Foundation

  /// Puts an app in the Trash and says where it went. A seam, so a test never fills the
  /// person's Trash.
  public protocol AppTrashing: Sendable {
    func trash(_ url: URL) throws -> URL
  }

  public struct SystemTrash: AppTrashing {
    public init() {}

    public func trash(_ url: URL) throws -> URL {
      var result: NSURL?
      try FileManager.default.trashItem(at: url, resultingItemURL: &result)
      return try Self.located(result)
    }

    /// Where the item went. The system reports it with the item already in the Trash; when it
    /// reports nothing, the item is there but cannot be put back from here.
    static func located(_ result: NSURL?) throws -> URL {
      guard let trashed = result as URL? else { throw AppMoveFailure.oldAppInTrash }
      return trashed
    }
  }

  /// Why a move did not happen. None of them leaves a half-written app at the destination.
  public enum AppMoveFailure: Error, Equatable, Sendable {
    /// The bundle to move is not this profile's app (its identifier differs, or it is not a
    /// bundle): nothing is read from or written to it.
    case notThisApp
    /// An app with the destination's name is there, and it is not this profile's app: it is
    /// never replaced.
    case destinationIsAnotherApp
    /// The app to move is the copy in Applications itself, reached by another path (a link, or
    /// a path that differs only in case).
    case alreadyThere
    /// A copy of this app is in an Applications folder that this account cannot write to.
    /// Writing the other folder would leave two copies, which is what the move exists to stop.
    case existingCopyLocked
    /// The copy in Applications is running.
    case destinationRunning
    /// An app of this profile is there and the person did not confirm replacing it.
    case replaceNotConfirmed
    /// The destination changed between the plan and the move.
    case planChanged
    /// Neither Applications folder can be written to.
    case noWritableFolder
    /// The copy did not finish; the staging copy was removed.
    case copyFailed
    /// The copy is not signed like the running app (its seal is broken, or someone else signed
    /// it); the staging copy was removed and the old app is where it was.
    case signatureRefused
    /// The old app could not be put in the Trash, or the new one could not take its place; the
    /// old app is where it was.
    case replaceFailed
    /// The old app is in the Trash and could not be put back, or the Trash did not say where it
    /// put it and the new app then could not take its place. (When the Trash did not say where and
    /// the new app took its place, the move succeeded.)
    case oldAppInTrash
  }

  /// What a move will do, shown to the person before it does anything.
  public struct AppMovePlan: Equatable, Sendable {
    public let source: URL
    public let destination: URL
    /// The folder of `destination` does not exist yet (the person's own `Applications`).
    public let createsFolder: Bool
    /// This profile's app is already at `destination`; it goes to the Trash once the person
    /// confirms, never to a hard delete.
    public let replacesExisting: Bool
    /// The source is left alone (a disk image or a translocated copy). Otherwise it goes to the
    /// Trash after the copy is in place, which makes the move a move.
    public let keepsSource: Bool
    /// The file that `destination` was when the plan was made. The Trash is given only that
    /// file: something else that lands there during the copy is never trashed.
    let destinationID: FileID?
  }

  /// A file on a volume, whatever path reaches it.
  struct FileID: Equatable, Sendable {
    let device: Int
    let inode: Int
  }

  /// Whether the app of `bundleId` runs from `app`. A seam, so a test starts no program.
  public protocol AppRunningCheck: Sendable {
    func isRunning(bundleId: String, at app: URL) -> Bool
  }

  /// Copies the running app to Applications. It reads and writes only bundles whose
  /// `CFBundleIdentifier` is the profile's own (`contracts/profiles.json`), so it cannot touch
  /// another app, the legacy one included, by any name.
  public struct AppMover: Sendable {
    let bundleId: String
    /// The destination's file name without `.app`, the profile's `macApp` name.
    let appName: String
    let folders: [URL]
    let trash: any AppTrashing
    let running: any AppRunningCheck
    let signature: any AppCodeValidating

    public init(
      bundleId: String, appName: String, folders: [URL], trash: any AppTrashing = SystemTrash(),
      running: any AppRunningCheck = WorkspaceRunningCheck(),
      signature: any AppCodeValidating = SystemCodeValidity()
    ) {
      self.bundleId = bundleId
      self.appName = appName
      self.folders = folders
      self.trash = trash
      self.running = running
      self.signature = signature
    }

    /// `Contents/Info.plist` of the bundle at `url`, read from the file each time (`Bundle` caches
    /// by path, and a replaced app keeps its path). Nil when there is none or it cannot be read.
    private static func info(at url: URL) -> [String: Any]? {
      do {
        let data = try Data(contentsOf: url.appending(path: "Contents/Info.plist"))
        return try PropertyListSerialization.propertyList(from: data, format: nil)
          as? [String: Any]
      } catch {
        return nil
      }
    }

    static func bundleIdentifier(at url: URL) -> String? {
      info(at: url)?["CFBundleIdentifier"] as? String
    }

    /// The file behind `url`, through links, so two paths to one app (a link, a path that
    /// differs in case on a case-insensitive volume) give the same answer.
    static func fileID(of url: URL) -> FileID? {
      do {
        let attributes = try FileManager.default.attributesOfItem(
          atPath: url.resolvingSymlinksInPath().path)
        guard let device = attributes[.systemNumber] as? Int,
          let inode = attributes[.systemFileNumber] as? Int
        else { return nil }
        return FileID(device: device, inode: inode)
      } catch {
        return nil
      }
    }

    /// Whether the bundle is a link to an app. `copyItem` would copy the link, and the person's
    /// folder would keep a link where the app was supposed to be.
    static func isLink(_ url: URL) -> Bool {
      do {
        return try url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink == true
      } catch {
        return false  // Unreadable: the plan reads the bundle next and refuses what is not an app.
      }
    }

    /// The bundle to copy: the app a link points to, else the bundle itself.
    static func copySource(_ url: URL) -> URL {
      isLink(url) ? url.resolvingSymlinksInPath() : url
    }

    private func isThisApp(_ url: URL) -> Bool {
      url.pathExtension == "app" && Self.bundleIdentifier(at: url) == bundleId
    }

    private func destination(in folder: URL) -> URL {
      folder.appending(path: appName + ".app", directoryHint: .isDirectory)
    }

    private enum FolderState { case ready, missing, unusable }

    /// A folder that exists and can be written, one that can be created, or neither.
    private func state(of folder: URL) -> FolderState {
      let files = FileManager.default
      var isFolder: ObjCBool = false
      if files.fileExists(atPath: folder.path, isDirectory: &isFolder) {
        return isFolder.boolValue && files.isWritableFile(atPath: folder.path) ? .ready : .unusable
      }
      return files.isWritableFile(atPath: folder.deletingLastPathComponent().path)
        ? .missing : .unusable
    }

    /// Where `source` would go and what that replaces, or why it cannot move. A copy of this app
    /// in any Applications folder is the destination, so the app never ends up in both.
    public func plan(source: URL, place: AppPlacement.Place) throws -> AppMovePlan {
      guard isThisApp(source) else { throw AppMoveFailure.notThisApp }
      let files = FileManager.default
      for folder in folders {
        let existing = destination(in: folder)
        if files.fileExists(atPath: existing.path), isThisApp(existing) {
          return try replacement(of: existing, in: folder, source: source, place: place)
        }
      }
      for folder in folders {
        let state = state(of: folder)
        guard state != .unusable else { continue }
        let target = destination(in: folder)
        // Never replace what is not this app, and never fall through to the next folder to
        // leave a second copy beside it.
        if files.fileExists(atPath: target.path) { throw AppMoveFailure.destinationIsAnotherApp }
        return AppMovePlan(
          source: source, destination: target, createsFolder: state == .missing,
          replacesExisting: false, keepsSource: keepsSource(source, place), destinationID: nil)
      }
      throw AppMoveFailure.noWritableFolder
    }

    private func replacement(
      of existing: URL, in folder: URL, source: URL, place: AppPlacement.Place
    ) throws -> AppMovePlan {
      let id = Self.fileID(of: existing)
      if id != nil, id == Self.fileID(of: source) { throw AppMoveFailure.alreadyThere }
      guard state(of: folder) == .ready else { throw AppMoveFailure.existingCopyLocked }
      if running.isRunning(bundleId: bundleId, at: existing) {
        throw AppMoveFailure.destinationRunning
      }
      return AppMovePlan(
        source: source, destination: existing, createsFolder: false, replacesExisting: true,
        keepsSource: keepsSource(source, place), destinationID: id)
    }

    /// The source is only read when it is a disk image's or a translocated copy, and when it is
    /// a link: the app it points to is not what the person opened, so it is not trashed.
    private func keepsSource(_ source: URL, _ place: AppPlacement.Place) -> Bool {
      place.isReadOnlyOrigin || Self.isLink(source)
    }

    /// Carries out `plan`: copies the app next to its destination under a staging name, checks
    /// the copy, puts the old app in the Trash, and renames the copy into place. The old app is
    /// put back when the rename fails. Returns the new app.
    @discardableResult
    public func move(_ plan: AppMovePlan, place: AppPlacement.Place, confirmedReplace: Bool)
      async throws -> URL
    {
      if plan.replacesExisting && !confirmedReplace { throw AppMoveFailure.replaceNotConfirmed }
      let again = try self.plan(source: plan.source, place: place)
      guard again.destination == plan.destination, again.replacesExisting == plan.replacesExisting,
        again.destinationID == plan.destinationID
      else { throw AppMoveFailure.planChanged }
      // Off the main actor: copying an app of this size takes seconds.
      return try await Task.detached(priority: .userInitiated) { try self.perform(again) }.value
    }

    private func perform(_ plan: AppMovePlan) throws -> URL {
      let files = FileManager.default
      let folder = plan.destination.deletingLastPathComponent()
      let staging = folder.appending(path: Self.stagingPrefix(appName) + UUID().uuidString)
      do {
        if plan.createsFolder {
          try files.createDirectory(at: folder, withIntermediateDirectories: false)
        }
        try files.copyItem(at: Self.copySource(plan.source), to: staging)
      } catch {
        Self.discard(staging)
        clientLogger.error("app move: copy failed kind=\(Self.kind(error), privacy: .public)")
        throw AppMoveFailure.copyFailed
      }
      guard isStagedCopy(staging) else {
        Self.discard(staging)
        throw AppMoveFailure.copyFailed
      }
      // Before the quarantine mark goes and before the old app is touched: a copy whose seal is
      // broken (cut short, or changed on the way), or that is not signed like the running app,
      // is never put in place.
      guard signature.isValid(staging) else {
        clientLogger.error("app move: the copy's signature is not valid")
        Self.discard(staging)
        throw AppMoveFailure.signatureRefused
      }
      Self.clearQuarantine(in: staging)
      // The copy took seconds: look at the destination again before anything is trashed.
      do { try confirmDestination(plan) } catch {
        Self.discard(staging)
        throw error
      }
      var trashed: URL?
      var oldAppLost = false
      do {
        if plan.replacesExisting {
          do {
            trashed = try trash.trash(plan.destination)
          } catch let failure as AppMoveFailure where failure == .oldAppInTrash {
            // The Trash took the old app and did not say where: it cannot be put back from here,
            // but the copy is whole and checked, so it still takes the place. Throwing it away
            // would leave nothing at the destination.
            clientLogger.error("app move: the Trash did not say where the old app went")
            oldAppLost = true
          }
        }
        try files.moveItem(at: staging, to: plan.destination)
      } catch {
        clientLogger.error("app move: replace failed kind=\(Self.kind(error), privacy: .public)")
        Self.discard(staging)
        if oldAppLost { throw AppMoveFailure.oldAppInTrash }
        if let trashed, !restore(trashed, to: plan.destination) {
          throw AppMoveFailure.oldAppInTrash
        }
        throw AppMoveFailure.replaceFailed
      }
      let sameFile = Self.fileID(of: plan.source) == Self.fileID(of: plan.destination)
      if !plan.keepsSource && !sameFile { trashOriginal(plan.source) }
      return plan.destination
    }

    /// The destination is still what the plan saw: the same copy of this app, not running, or
    /// still nothing.
    private func confirmDestination(_ plan: AppMovePlan) throws {
      let exists = FileManager.default.fileExists(atPath: plan.destination.path)
      guard exists == plan.replacesExisting else { throw AppMoveFailure.planChanged }
      guard plan.replacesExisting else { return }
      if running.isRunning(bundleId: bundleId, at: plan.destination) {
        throw AppMoveFailure.destinationRunning
      }
      guard isThisApp(plan.destination), Self.fileID(of: plan.destination) == plan.destinationID
      else { throw AppMoveFailure.planChanged }
    }

    /// The copy is complete enough to check: same identifier, and the executable is there.
    private func isStagedCopy(_ url: URL) -> Bool {
      guard
        let info = Self.info(at: url),
        info["CFBundleIdentifier"] as? String == bundleId,
        let executable = info["CFBundleExecutable"] as? String
      else { return false }
      return FileManager.default.isExecutableFile(
        atPath: url.appending(path: "Contents/MacOS/\(executable)").path)
    }

    /// Takes the quarantine mark off the copy. `copyItem` rewrites it into a mark without the
    /// origin, which makes the copy look like a new download to Gatekeeper. The person has already
    /// opened this app (it is the running one, after the check that ran for the download), the
    /// release is notarized, and the mark would make macOS check the copy again at the next
    /// launch, from a place the person chose to trust. A failure is logged and the move goes on:
    /// the mark is a warning, not a part of the app.
    static func clearQuarantine(in bundle: URL) {
      let mark = "com.apple.quarantine"
      var items = [bundle]
      if let walk = FileManager.default.enumerator(at: bundle, includingPropertiesForKeys: nil) {
        items += walk.compactMap { $0 as? URL }
      }
      var failures = 0
      for item in items {
        let status = item.withUnsafeFileSystemRepresentation { path in
          path.map { removexattr($0, mark, XATTR_NOFOLLOW) } ?? -1
        }
        if status != 0 && errno != ENOATTR { failures += 1 }
      }
      if failures > 0 {
        clientLogger.error("app move: quarantine kept on \(failures, privacy: .public) items")
      }
    }

    /// The original of an app that was not on a disk image: a move leaves nothing behind. A
    /// failure is only logged, since the new app is already in place.
    private func trashOriginal(_ source: URL) {
      do {
        _ = try trash.trash(source)
      } catch {
        clientLogger.error("app move: original kept kind=\(Self.kind(error), privacy: .public)")
      }
    }

    private func restore(_ trashed: URL, to destination: URL) -> Bool {
      do {
        try FileManager.default.moveItem(at: trashed, to: destination)
        return true
      } catch {
        clientLogger.error("app move: restore failed kind=\(Self.kind(error), privacy: .public)")
        return false
      }
    }

    private static func stagingPrefix(_ appName: String) -> String { ".\(appName).moving." }

    /// Removes the hidden copies a move that was cut short (a crash, a power cut) left in the
    /// Applications folders. Only a name this mover makes, with a UUID, is touched.
    public func removeLeftovers() {
      let prefix = Self.stagingPrefix(appName)
      for folder in folders {
        let names: [String]
        do {
          names = try FileManager.default.contentsOfDirectory(atPath: folder.path)
        } catch {
          continue  // The folder does not exist (yet): nothing was left in it.
        }
        for name in names where name.hasPrefix(prefix) {
          if UUID(uuidString: String(name.dropFirst(prefix.count))) != nil {
            Self.discard(folder.appending(path: name))
          }
        }
      }
    }

    private static func discard(_ staging: URL) {
      guard FileManager.default.fileExists(atPath: staging.path) else { return }
      do {
        try FileManager.default.removeItem(at: staging)
      } catch {
        clientLogger.error("app move: cleanup failed kind=\(kind(error), privacy: .public)")
      }
    }

    /// The error's domain and code, never its text, which can carry a path.
    private static func kind(_ error: any Error) -> String {
      let value = error as NSError
      return "\(value.domain)/\(value.code)"
    }
  }
#endif
