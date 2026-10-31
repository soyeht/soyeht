#if os(macOS)
  import Foundation
  import MetricKit
  import Observation
  import SoyehtProtocol

  /// The quiet notice "Soyeht closed unexpectedly" (ADR 0127, contract 16): the crash reports the
  /// person has not seen, "Copy report" (which is "Copy diagnostics", since the report is part of
  /// it) and "Dismiss". It asks the bundled `soyeht` program, as the diagnostics do, and sends
  /// nothing anywhere.
  @MainActor @Observable
  public final class CrashNotice {
    /// What `soyeht crash-reports status` prints.
    struct Status: Decodable, Equatable {
      let reports: [String]
    }

    /// Whether the notice is on screen: there is a report the person has not seen.
    public private(set) var isShowing = false
    @ObservationIgnored private let command: BundledEngineService?
    @ObservationIgnored private let dataDirectory: URL
    @ObservationIgnored private var subscriber: CrashDiagnosticSubscriber?
    /// The reports the notice shows, which Dismiss acknowledges: a report that arrives while it is
    /// up is not hidden by it.
    @ObservationIgnored private var shown: [String] = []

    public convenience init() {
      self.init(
        command: BundledEngineService.forDiagnostics(), dataDirectory: ActiveProfile.dataDirectory)
    }

    public init(command: BundledEngineService?, dataDirectory: URL) {
      self.command = command
      self.dataDirectory = dataDirectory
    }

    /// Starts listening for the system's crash diagnostics and looks for reports not yet seen.
    public func start() async {
      if subscriber == nil {
        // A diagnostic left by a run that ended before the program read it holds free text.
        Self.sweepIncoming(in: dataDirectory)
        let subscriber = CrashDiagnosticSubscriber { [weak self] diagnostics in
          await self?.receive(diagnostics)
        }
        MXMetricManager.shared.add(subscriber)
        self.subscriber = subscriber
      }
      await refresh()
    }

    /// Asks the program how many reports are pending. A program that is missing or fails shows
    /// nothing: the notice is a courtesy, and the failure is logged by its kind.
    public func refresh() async {
      guard let command else { return }
      do {
        let status = try JSONDecoder().decode(
          Status.self, from: try await command.crashReports(["status"]))
        shown = status.reports
        isShowing = !shown.isEmpty
      } catch {
        clientLogger.notice("mac.crash_notice_failed kind=status")
      }
    }

    /// "Copy report": the diagnostics, which hold the report, go to the clipboard; once they have,
    /// the person has seen it and the notice goes. If the copy fails, the notice stays.
    public func copyReport(using diagnostics: DiagnosticsCopy) async {
      await diagnostics.copy()
      if diagnostics.phase == .copied { await dismiss() }
    }

    /// "Dismiss": the reports stay for the diagnostics, and the notice does not come back for
    /// them.
    public func dismiss() async {
      isShowing = false
      let acknowledged = shown
      shown = []
      guard let command, !acknowledged.isEmpty else { return }
      do {
        _ = try await command.crashReports(["acknowledge"] + acknowledged)
      } catch {
        clientLogger.notice("mac.crash_notice_failed kind=acknowledge")
      }
    }

    /// Hands the system's crash diagnostics (one JSON text each) to the program, which keeps the
    /// facts of contract 16 section 5 and writes the report, then shows the notice.
    func receive(_ diagnostics: [Data]) async {
      guard let command else { return }
      for diagnostic in diagnostics {
        var file: URL?
        do {
          file = try Self.drop(diagnostic, in: dataDirectory)
          if let file { _ = try await command.crashReports(["import", file.path]) }
        } catch {
          clientLogger.notice("mac.crash_notice_failed kind=import")
          // The program deletes what it imports; what it refused, or never saw, is deleted here.
          Self.discard(file)
        }
      }
      await refresh()
    }

    static func discard(_ file: URL?) {
      guard let file else { return }
      do {
        try FileManager.default.removeItem(at: file)
      } catch CocoaError.fileNoSuchFile {
        // The program had already deleted it.
      } catch {
        clientLogger.notice("mac.crash_notice_failed kind=discard")
      }
    }

    /// Removes the `incoming-*` files in `crashes/`: diagnostics no run is waiting on.
    static func sweepIncoming(in dataDirectory: URL) {
      let folder = dataDirectory.appending(path: "crashes", directoryHint: .isDirectory)
      let names: [String]
      do {
        names = try FileManager.default.contentsOfDirectory(atPath: folder.path)
      } catch {
        // No folder yet holds nothing to sweep.
        return
      }
      for name in names where name.hasPrefix("incoming-") {
        discard(folder.appending(path: name))
      }
    }

    /// Writes a diagnostic where the program reads it (`crashes/` of the data directory,
    /// owner-only); the program deletes it once imported.
    static func drop(_ diagnostic: Data, in dataDirectory: URL) throws -> URL {
      let folder = dataDirectory.appending(path: "crashes", directoryHint: .isDirectory)
      try FileManager.default.createDirectory(
        at: folder, withIntermediateDirectories: true,
        attributes: [.posixPermissions: 0o700])
      let file = folder.appending(path: "incoming-\(UUID().uuidString).json")
      try diagnostic.write(to: file, options: .atomic)
      try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
      return file
    }
  }

  /// Receives the crash diagnostics the system delivers on a launch after a crash.
  final class CrashDiagnosticSubscriber: NSObject, MXMetricManagerSubscriber, @unchecked Sendable {
    private let deliver: @Sendable ([Data]) async -> Void

    init(deliver: @escaping @Sendable ([Data]) async -> Void) { self.deliver = deliver }

    func didReceive(_ payloads: [MXDiagnosticPayload]) {
      let crashes = payloads.flatMap { $0.crashDiagnostics ?? [] }.map { $0.jsonRepresentation() }
      guard !crashes.isEmpty else { return }
      let deliver = deliver
      Task { await deliver(crashes) }
    }
  }
#endif
