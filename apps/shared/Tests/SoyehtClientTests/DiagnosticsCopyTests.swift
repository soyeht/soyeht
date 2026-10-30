#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import os
  import Testing

  @testable import SoyehtClient

  /// Answers the `soyeht diagnostics` call from a script and records what was asked.
  private final class ScriptedReport: ServiceCommandRunning, Sendable {
    private let calls = OSAllocatedUnfairLock<[(URL, [String])]>(initialState: [])
    private let answer: Result<Data, ServiceCommandError>

    init(_ answer: Result<Data, ServiceCommandError>) { self.answer = answer }

    var asked: [(URL, [String])] { calls.withLock { $0 } }

    func run(_ executable: URL, arguments: [String]) async throws -> Data {
      calls.withLock { $0.append((executable, arguments)) }
      return try answer.get()
    }
  }

  private let report = "## Soyeht diagnostics\n\n- engine: not running\n"
  private let bundled = URL(fileURLWithPath: "/Applications/Soyeht Next.app/Contents/MacOS/soyeht")
  private let data = URL(fileURLWithPath: "/Users/person/Library/Application Support/Soyeht.next")

  private func command(_ runner: ScriptedReport) -> BundledEngineService {
    BundledEngineService(command: bundled, profile: .prod, dataDirectory: data, runner: runner)
  }

  /// What the action wrote to the clipboard.
  private final class Clipboard: @unchecked Sendable {
    var written: [String] = []
  }

  @MainActor private func action(_ runner: ScriptedReport?, _ clipboard: Clipboard)
    -> DiagnosticsCopy
  {
    DiagnosticsCopy(
      command: runner.map(command), write: { clipboard.written.append($0) })
  }

  @Test func theReportIsWhatTheProgramPrintsForTheProfileAndDataDirectory() async throws {
    let runner = ScriptedReport(.success(Data(report.utf8)))
    let text = try await command(runner).diagnosticsReport()
    #expect(text == report)
    #expect(runner.asked.count == 1)
    #expect(runner.asked[0].0 == bundled)
    #expect(
      runner.asked[0].1 == ["--profile", "prod", "--data-dir", data.path, "diagnostics"])
  }

  @Test func textThatIsNotUTF8IsRefused() async {
    let runner = ScriptedReport(.success(Data([0xFF, 0xFE])))
    await #expect(throws: ServiceCommandError.unreadable) {
      try await command(runner).diagnosticsReport()
    }
  }

  @Test @MainActor func copyingPutsTheReportOnTheClipboardAndConfirms() async {
    let clipboard = Clipboard()
    let copy = action(ScriptedReport(.success(Data(report.utf8))), clipboard)
    #expect(copy.message == nil)
    await copy.copy()
    #expect(clipboard.written == [report])
    #expect(copy.phase == .copied)
    #expect(copy.message == ClientPolicy.diagnosticsCopied)
    #expect(!copy.isFailure)
  }

  @Test @MainActor func aFailureIsShownAndNothingIsCopied() async {
    for failure in [
      ServiceCommandError.failed(status: 2, detail: "boom /Users/person/x"),
      .timedOut, .unreadable,
    ] {
      let clipboard = Clipboard()
      let copy = action(ScriptedReport(.failure(failure)), clipboard)
      await copy.copy()
      #expect(clipboard.written.isEmpty)
      #expect(copy.phase == .failed)
      #expect(copy.message == ClientPolicy.diagnosticsFailed)
      #expect(copy.isFailure)
      #expect(copy.message?.contains("person") == false)
    }
  }

  @Test @MainActor func aCopyWithoutTheProgramSaysSo() async {
    let clipboard = Clipboard()
    let copy = action(nil, clipboard)
    await copy.copy()
    #expect(clipboard.written.isEmpty)
    #expect(copy.phase == .unavailable)
    #expect(copy.message == ClientPolicy.diagnosticsUnavailable)
    #expect(copy.isFailure)
  }

  @Test @MainActor func copyingAgainCopiesAgain() async {
    let clipboard = Clipboard()
    let copy = action(ScriptedReport(.success(Data(report.utf8))), clipboard)
    await copy.copy()
    await copy.copy()
    #expect(clipboard.written == [report, report])
    #expect(copy.phase == .copied)
  }

  @Test func aProgramBesideTheAppIsFoundInTheBundleOrTheDevFolder() throws {
    let folder = FileManager.default.temporaryDirectory
      .appending(path: "diagnostics-\(UUID().uuidString)", directoryHint: .isDirectory)
    defer { try? FileManager.default.removeItem(at: folder) }
    let bin = folder.appending(path: "bin", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
    let empty = Bundle(path: folder.path) ?? Bundle.main
    #expect(
      BundledEngineService.forDiagnostics(bundle: empty, profile: .dev, dataDirectory: folder)
        == nil)
    let installed = bin.appending(path: "soyeht")
    try Data("#!/bin/sh\n".utf8).write(to: installed)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: installed.path)
    #expect(
      BundledEngineService.forDiagnostics(bundle: empty, profile: .dev, dataDirectory: folder)
        != nil)
  }
#endif
