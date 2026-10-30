#if os(macOS)
  import AppKit
  import Foundation
  import Observation
  import SoyehtProtocol

  extension BundledEngineService {
    /// The `soyeht` program that builds the diagnostics report: the one in the app's bundle or,
    /// for a development build, which carries none, the copy the Dev installer put in the
    /// profile's `bin` folder. Nil when this copy of the app has neither.
    public static func forDiagnostics(
      bundle: Bundle = .main, profile: ProfileValues = ActiveProfile.values,
      dataDirectory: URL = ActiveProfile.dataDirectory,
      runner: any ServiceCommandRunning = ProcessRunner(
        seconds: Double(ClientPolicy.diagnosticsMs) / 1000,
        maxOutputBytes: ClientPolicy.capsDiagnosticsReportBytes)
    ) -> BundledEngineService? {
      if let bundled = BundledEngineService(
        bundle: bundle, profile: profile, dataDirectory: dataDirectory, runner: runner)
      {
        return bundled
      }
      let installed = dataDirectory.appending(path: "bin", directoryHint: .isDirectory)
        .appending(path: commandName)
      guard FileManager.default.isExecutableFile(atPath: installed.path) else { return nil }
      return BundledEngineService(
        command: installed, profile: profile, dataDirectory: dataDirectory, runner: runner)
    }
  }

  /// The "Copy diagnostics" action: collects the report, puts it on the clipboard and says how
  /// that went, with the sentences of `x-clientPolicy` that the desktop app shows too.
  @MainActor @Observable
  public final class DiagnosticsCopy {
    public enum Phase: Equatable, Sendable {
      case idle
      case working
      /// The report is on the clipboard.
      case copied
      /// This copy of the app has no `soyeht` program.
      case unavailable
      case failed
    }

    public private(set) var phase = Phase.idle
    @ObservationIgnored private let command: BundledEngineService?
    @ObservationIgnored private let write: @MainActor (String) -> Void

    public convenience init() {
      self.init(
        command: BundledEngineService.forDiagnostics(),
        write: { text in
          NSPasteboard.general.clearContents()
          NSPasteboard.general.setString(text, forType: .string)
        })
    }

    public init(command: BundledEngineService?, write: @escaping @MainActor (String) -> Void) {
      self.command = command
      self.write = write
    }

    /// The sentence under the button, if the action has one to say.
    public var message: String? {
      switch phase {
      case .idle: nil
      case .working: ClientPolicy.diagnosticsWorking
      case .copied: ClientPolicy.diagnosticsCopied
      case .unavailable: ClientPolicy.diagnosticsUnavailable
      case .failed: ClientPolicy.diagnosticsFailed
      }
    }

    public var isFailure: Bool { phase == .failed || phase == .unavailable }

    /// Collects the report and puts it on the clipboard, unless a collection is already running.
    public func copy() async {
      guard phase != .working else { return }
      guard let command else {
        phase = .unavailable
        clientLogger.notice("mac.diagnostics_failed kind=unavailable")
        return
      }
      phase = .working
      do {
        write(try await command.diagnosticsReport())
        phase = .copied
      } catch {
        // The kind only: the program's message can carry a path.
        clientLogger.notice("mac.diagnostics_failed kind=\(Self.kind(error), privacy: .public)")
        phase = .failed
      }
    }

    private static func kind(_ error: any Error) -> String {
      switch error {
      case ServiceCommandError.failed(let status, _): "status:\(status)"
      case ServiceCommandError.timedOut: "timed_out"
      case ServiceCommandError.unreadable: "not_text"
      case ServiceCommandError.tooLarge: "too_large"
      default: "run"
      }
    }
  }
#endif
