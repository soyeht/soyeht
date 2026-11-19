#if os(macOS)
  import Foundation
  import Testing
  @testable import SoyehtClient

  /// The supervisor and the engine of a disposable data directory, started the way the programs
  /// of a build start; never the installed Dev socket. Both Swift integration tests use it.
  @MainActor final class EngineFixture {
    let directory: URL
    let binaries: URL
    let log: URL
    private let output: FileHandle
    private var supervisor: Process?
    var engine: Process?

    var socketPath: String { directory.appendingPathComponent("engine.sock").path }
    /// Everything the programs wrote to their standard output and error: the engine's log lines.
    var logText: String { (try? String(contentsOf: log, encoding: .utf8)) ?? "" }

    /// `nil`, with an issue recorded, when the Rust workspace was not built.
    init?() {
      let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent()
      binaries =
        ProcessInfo.processInfo.environment["SOYEHT_TEST_BIN_DIR"]
        .map { URL(fileURLWithPath: $0) } ?? root.appendingPathComponent("engine/target/debug")
      guard
        FileManager.default.isExecutableFile(
          atPath: binaries.appendingPathComponent("soyeht-engine").path)
      else {
        Issue.record("Build the Rust workspace before the native client integration test.")
        return nil
      }
      directory = URL(fileURLWithPath: "/tmp/sn-sw-" + UUID().uuidString.prefix(8))
      log = directory.appendingPathComponent("test.log")
      do {
        try FileManager.default.createDirectory(
          at: directory, withIntermediateDirectories: true,
          attributes: [.posixPermissions: 0o700])
        try "PROMPT='SWIFT_READY> '\nHISTFILE=\nSAVEHIST=0\n".write(
          to: directory.appendingPathComponent(".zshrc"), atomically: true, encoding: .utf8)
        FileManager.default.createFile(atPath: log.path, contents: nil)
        output = try FileHandle(forWritingTo: log)
      } catch {
        Issue.record("Fixture directory: \(error)")
        return nil
      }
    }

    func launch(_ name: String) throws -> Process {
      let process = Process()
      process.executableURL = binaries.appendingPathComponent(name)
      process.arguments = ["--data-dir", directory.path]
      if name == "soyeht-engine" { process.arguments! += ["--listen", "127.0.0.1:0"] }
      process.environment = ProcessInfo.processInfo.environment
      process.environment?["ZDOTDIR"] = directory.path
      process.standardOutput = output
      process.standardError = output
      try process.run()
      return process
    }

    /// The supervisor first, then the engine once the supervisor's socket exists.
    func start() async throws {
      supervisor = try launch("soyeht-ptyd")
      try await until {
        FileManager.default.fileExists(
          atPath: self.directory.appendingPathComponent("ptyd.sock").path)
      }
      engine = try launch("soyeht-engine")
    }

    /// Closes the clients' panes with the command line, stops the clients and the programs, and
    /// removes the directory.
    func tearDown(_ clients: EngineClient...) {
      if engine?.isRunning == true {
        for pane in clients.flatMap(\.panes) {
          let close = Process()
          close.executableURL = binaries.appendingPathComponent("soyeht")
          close.arguments = [
            "--profile", "dev", "--data-dir", directory.path, "pane", "close", pane.paneId,
          ]
          close.standardOutput = output
          close.standardError = output
          if (try? close.run()) != nil { close.waitUntilExit() }
        }
      }
      for client in clients { client.stop() }
      engine?.terminate()
      supervisor?.terminate()
      try? output.close()
      // The supervisor only owns this fixture's children. Normal close is tested elsewhere.
      if engine?.isRunning == true { engine?.waitUntilExit() }
      if supervisor?.isRunning == true { supervisor?.waitUntilExit() }
      try? FileManager.default.removeItem(at: directory)
    }
  }

  /// Waits for `condition`, polling every 100 ms, for up to `seconds`; a deadline, never an assertion.
  @MainActor func until(seconds: Int = 30, _ condition: () -> Bool) async throws {
    for _ in 0..<(seconds * 10) {
      if condition() { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw ClientFailure.timeout
  }
#endif
