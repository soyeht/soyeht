#if os(macOS)
  import Foundation
  import Observation
  import SoyehtProtocol

  /// What `soyeht service` reports for one component (`docs/contracts/13-service-seam.md`).
  public struct ServiceState: Decodable, Sendable, Equatable {
    public let component: String
    public let name: String
    public let registered: Bool
    public let running: Bool
    /// Only in the answer to `status --programs`: the definition on disk is exactly what
    /// `install` would write for those programs.
    public let current: Bool?
  }

  public enum ServiceCommandError: Error, Equatable, Sendable {
    case failed(status: Int32, detail: String)
    case timedOut
    case unreadable
    /// The program printed more than the caller allows.
    case tooLarge
  }

  /// Runs the bundled command line. A seam so tests run no program.
  public protocol ServiceCommandRunning: Sendable {
    func run(_ executable: URL, arguments: [String]) async throws -> Data
  }

  /// The engine and the supervisor that ship inside this app (ADR 0120). The app does not
  /// register them itself: it runs the bundled `soyeht service ...`, the one writer of a service
  /// definition (ADR 0114), with `--programs` set to its own bundle.
  public struct BundledEngineService: Sendable {
    public static let commandName = "soyeht"
    /// How long one `soyeht service` call may take before it is ended.
    public static let commandSeconds = 60.0

    let command: URL
    let profile: ProfileValues
    let dataDirectory: URL
    let runner: any ServiceCommandRunning

    /// Nil when the bundle carries no command line: a development build, which the Dev
    /// installer registers instead.
    public init?(
      bundle: Bundle = .main, profile: ProfileValues = ActiveProfile.values,
      dataDirectory: URL = ActiveProfile.dataDirectory,
      runner: any ServiceCommandRunning = ProcessRunner()
    ) {
      guard let command = bundle.url(forAuxiliaryExecutable: Self.commandName) else {
        return nil
      }
      self.init(
        command: command, profile: profile, dataDirectory: dataDirectory, runner: runner)
    }

    init(
      command: URL, profile: ProfileValues, dataDirectory: URL,
      runner: any ServiceCommandRunning
    ) {
      self.command = command
      self.profile = profile
      self.dataDirectory = dataDirectory
      self.runner = runner
    }

    /// The folder the programs sit in: the one the command line sits in.
    var programs: URL { command.deletingLastPathComponent() }

    /// A registration points at this bundle's path, so a copy that macOS may move or remove
    /// must not be registered: one running from a disk image (read-only, and gone when it is
    /// ejected) or from an App Translocation path (a random read-only copy of a downloaded app
    /// that has not been moved).
    public var runsFromStablePlace: Bool { Self.isStablePlace(command) }

    static func isStablePlace(_ url: URL) -> Bool {
      let path = url.standardizedFileURL.path
      return !path.hasPrefix("/Volumes/") && !path.contains("/AppTranslocation/")
    }

    /// What every call of the command line for this profile starts with.
    private var profileArguments: [String] {
      ["--profile", profile.name, "--data-dir", dataDirectory.path]
    }

    func arguments(_ action: [String]) -> [String] {
      profileArguments + ["service"] + action
    }

    /// The report `soyeht diagnostics` prints (ADR 0118, contract 14 section 4), as it printed
    /// it: the app adds no fact and no redaction rule, and the report needs no engine.
    public func diagnosticsReport() async throws -> String {
      let output = try await runner.run(command, arguments: profileArguments + ["diagnostics"])
      guard let text = String(data: output, encoding: .utf8) else {
        throw ServiceCommandError.unreadable
      }
      return text
    }

    private func services(_ action: [String]) async throws -> [ServiceState] {
      let output = try await runner.run(command, arguments: arguments(action))
      do {
        return try JSONDecoder().decode(Report.self, from: output).services
      } catch {
        throw ServiceCommandError.unreadable
      }
    }

    private struct Report: Decodable { let services: [ServiceState] }

    /// The services as registered, each with whether its definition names this bundle's programs.
    public func status() async throws -> [ServiceState] {
      try await services(["status", "--programs", programs.path])
    }

    /// Writes the definitions for this bundle's path, registers and starts them. Running it
    /// again is the repair after the app moved: a program that runs is left running, and one
    /// that is loaded but idle is loaded again with the new path (contract 13, section 3).
    @discardableResult
    public func install() async throws -> [ServiceState] {
      try await services(["install", "--programs", programs.path])
    }
  }

  /// Runs a program and returns what it printed; a non-zero status carries what it said. A
  /// program that runs past `seconds` is ended, and so is one that prints more than
  /// `maxOutputBytes` (the diagnostics report is bounded, contract 14 section 4).
  public struct ProcessRunner: ServiceCommandRunning {
    let seconds: Double
    let maxOutputBytes: Int?

    public init(seconds: Double = BundledEngineService.commandSeconds, maxOutputBytes: Int? = nil) {
      self.seconds = seconds
      self.maxOutputBytes = maxOutputBytes
    }

    public func run(_ executable: URL, arguments: [String]) async throws -> Data {
      let (seconds, limit) = (seconds, maxOutputBytes)
      return try await withCheckedThrowingContinuation { continuation in
        DispatchQueue.global(qos: .userInitiated).async {
          continuation.resume(
            with: Result { try Self.runBlocking(executable, arguments, seconds, limit) })
        }
      }
    }

    private final class Captured: @unchecked Sendable {
      var data = Data()
      var exceeded = false
    }

    /// Reads `pipe` to its end. What passes `limit` is dropped and ends `process`.
    private static func drain(_ pipe: Pipe, into sink: Captured, limit: Int?, process: Process) {
      let handle = pipe.fileHandleForReading
      while true {
        let chunk = handle.availableData
        if chunk.isEmpty { return }
        if let limit, sink.data.count + chunk.count > limit {
          if !sink.exceeded {
            sink.exceeded = true
            process.terminate()
          }
          continue
        }
        sink.data.append(chunk)
      }
    }

    private static func runBlocking(
      _ executable: URL, _ arguments: [String], _ seconds: Double, _ limit: Int?
    ) throws -> Data {
      let process = Process()
      process.executableURL = executable
      process.arguments = arguments
      let output = Pipe()
      let errors = Pipe()
      process.standardOutput = output
      process.standardError = errors
      let finished = DispatchSemaphore(value: 0)
      process.terminationHandler = { _ in finished.signal() }
      try process.run()
      let printed = Captured()
      let said = Captured()
      let group = DispatchGroup()
      for (pipe, sink, cap) in [(output, printed, limit), (errors, said, nil)] {
        DispatchQueue.global().async(group: group) {
          drain(pipe, into: sink, limit: cap, process: process)
        }
      }
      let timedOut = finished.wait(timeout: .now() + seconds) == .timedOut
      if timedOut { process.terminate() }
      group.wait()
      if timedOut { throw ServiceCommandError.timedOut }
      if printed.exceeded { throw ServiceCommandError.tooLarge }
      guard process.terminationStatus == 0 else {
        let detail = String(decoding: said.data, as: UTF8.self)
          .trimmingCharacters(in: .whitespacesAndNewlines)
        throw ServiceCommandError.failed(status: process.terminationStatus, detail: detail)
      }
      return printed.data
    }
  }

  /// Whether the engine is set up to start at login, and the one question the app asks to do it.
  /// Registering a background program is the person's choice (contract 13, section 3): the app
  /// asks once, and the setup list keeps the button for a person who said no.
  @MainActor @Observable
  public final class EngineServiceSetup {
    public enum Phase: Equatable, Sendable {
      /// This copy carries no programs (a development build).
      case unavailable
      case checking
      /// Registered, and the definitions point at this bundle.
      case ready
      /// Not registered: ask the person.
      case offered
      case working
      /// The app runs from a disk image or a translocated path: move it to Applications first.
      case moveFirst
      case failed(String)
    }

    public static let declinedKey = "soyeht.engineService.declined"

    public private(set) var phase: Phase
    @ObservationIgnored private let service: BundledEngineService?
    @ObservationIgnored private let preferences: any ClientPreferenceStore

    public convenience init() {
      self.init(service: BundledEngineService(), preferences: UserDefaultsClientPreferenceStore())
    }

    public init(service: BundledEngineService?, preferences: any ClientPreferenceStore) {
      self.service = service
      self.preferences = preferences
      phase = service == nil ? .unavailable : .checking
    }

    /// Whether the app should put its question to the person now.
    public var shouldAsk: Bool {
      phase == .offered && !preferences.bool(forKey: Self.declinedKey)
    }

    /// Whether the setup list should offer to start Soyeht. The caller shows it only while the
    /// engine is not reachable, so `.ready` counts: services that are registered and right but
    /// not answering (stopped by the person, crashed, or not loaded again by the system) get the
    /// explicit button. Nothing starts them at launch, so a stop is respected.
    public var offersStart: Bool {
      switch phase {
      case .offered, .ready, .failed, .moveFirst: true
      case .unavailable, .checking, .working: false
      }
    }

    /// At launch: when the services are registered for another path than this bundle's, write them
    /// again for this one, which repairs the registration of an app that was moved. A
    /// registration that is right is left alone, so a person who stopped the services with
    /// `soyeht service stop` does not find them started at the next launch, and the system does
    /// not announce a background item every time. When nothing is registered, offer.
    public func refresh() async {
      guard let service else { return }
      guard service.runsFromStablePlace else {
        phase = .moveFirst
        return
      }
      phase = .checking
      do {
        let states = try await service.status()
        let registered = states.filter(\.registered)
        if registered.isEmpty {
          phase = .offered
        } else {
          // A service that is missing from a registration the person made (a deleted file, an
          // install that stopped half way) is as stale as one that names another path.
          let stale = registered.count < states.count || registered.contains { $0.current != true }
          if stale { try await service.install() }
          phase = .ready
        }
      } catch {
        phase = .failed(Self.describe(error))
      }
    }

    /// The person agreed: register and start the services.
    public func enable() async {
      guard let service, service.runsFromStablePlace else { return }
      preferences.set(false, forKey: Self.declinedKey)
      phase = .working
      do {
        try await service.install()
        phase = .ready
      } catch {
        phase = .failed(Self.describe(error))
      }
    }

    public func decline() {
      preferences.set(true, forKey: Self.declinedKey)
    }

    static func describe(_ error: any Error) -> String {
      switch error {
      case ServiceCommandError.failed(_, let detail) where !detail.isEmpty: detail
      case ServiceCommandError.timedOut: "Starting Soyeht took too long."
      default: "Soyeht could not set up its background services."
      }
    }
  }
#endif
