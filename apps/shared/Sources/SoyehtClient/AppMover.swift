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
      guard let trashed = result as URL? else { throw AppMoveFailure.replaceFailed }
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
    /// An app of this profile is there and the person did not confirm replacing it.
    case replaceNotConfirmed
    /// The destination changed between the plan and the move.
    case planChanged
    /// Neither Applications folder can be written to.
    case noWritableFolder
    /// The copy did not finish; the staging copy was removed.
    case copyFailed
    /// The old app could not be put in the Trash, or the new one could not take its place; the
    /// old app is where it was.
    case replaceFailed
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

    public init(
      bundleId: String, appName: String, folders: [URL], trash: any AppTrashing = SystemTrash()
    ) {
      self.bundleId = bundleId
      self.appName = appName
      self.folders = folders
      self.trash = trash
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

    private func isThisApp(_ url: URL) -> Bool {
      url.pathExtension == "app" && Self.bundleIdentifier(at: url) == bundleId
    }

    /// Where `source` would go and what that replaces, or why it cannot move.
    public func plan(source: URL, place: AppPlacement.Place) throws -> AppMovePlan {
      guard isThisApp(source) else { throw AppMoveFailure.notThisApp }
      let files = FileManager.default
      for folder in folders {
        let destination = folder.appending(path: appName + ".app", directoryHint: .isDirectory)
        var isFolder: ObjCBool = false
        let exists = files.fileExists(atPath: folder.path, isDirectory: &isFolder)
        if exists {
          guard isFolder.boolValue, files.isWritableFile(atPath: folder.path) else { continue }
        } else {
          let parent = folder.deletingLastPathComponent()
          guard files.isWritableFile(atPath: parent.path) else { continue }
        }
        var replaces = false
        if files.fileExists(atPath: destination.path) {
          // Never replace what is not this app, and never fall through to the next folder to
          // leave a second copy beside it.
          guard isThisApp(destination) else { throw AppMoveFailure.destinationIsAnotherApp }
          replaces = true
        }
        return AppMovePlan(
          source: source, destination: destination, createsFolder: !exists,
          replacesExisting: replaces, keepsSource: place.isReadOnlyOrigin)
      }
      throw AppMoveFailure.noWritableFolder
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
      guard again.destination == plan.destination, again.replacesExisting == plan.replacesExisting
      else { throw AppMoveFailure.planChanged }
      // Off the main actor: copying an app of this size takes seconds.
      return try await Task.detached(priority: .userInitiated) { try self.perform(again) }.value
    }

    private func perform(_ plan: AppMovePlan) throws -> URL {
      let files = FileManager.default
      let folder = plan.destination.deletingLastPathComponent()
      let staging = folder.appending(path: ".\(appName).moving.\(UUID().uuidString)")
      do {
        if plan.createsFolder {
          try files.createDirectory(at: folder, withIntermediateDirectories: false)
        }
        try files.copyItem(at: plan.source, to: staging)
      } catch {
        Self.discard(staging)
        clientLogger.error("app move: copy failed kind=\(Self.kind(error), privacy: .public)")
        throw AppMoveFailure.copyFailed
      }
      guard isStagedCopy(staging) else {
        Self.discard(staging)
        throw AppMoveFailure.copyFailed
      }
      var trashed: URL?
      do {
        if plan.replacesExisting { trashed = try trash.trash(plan.destination) }
        try files.moveItem(at: staging, to: plan.destination)
      } catch {
        clientLogger.error("app move: replace failed kind=\(Self.kind(error), privacy: .public)")
        Self.discard(staging)
        if let trashed { restore(trashed, to: plan.destination) }
        throw AppMoveFailure.replaceFailed
      }
      if !plan.keepsSource { trashOriginal(plan.source) }
      return plan.destination
    }

    /// The copy is complete enough to launch: same identifier, and the executable is there.
    private func isStagedCopy(_ url: URL) -> Bool {
      guard
        let info = Self.info(at: url),
        info["CFBundleIdentifier"] as? String == bundleId,
        let executable = info["CFBundleExecutable"] as? String
      else { return false }
      return FileManager.default.isExecutableFile(
        atPath: url.appending(path: "Contents/MacOS/\(executable)").path)
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

    private func restore(_ trashed: URL, to destination: URL) {
      do {
        try FileManager.default.moveItem(at: trashed, to: destination)
      } catch {
        clientLogger.error("app move: restore failed kind=\(Self.kind(error), privacy: .public)")
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
