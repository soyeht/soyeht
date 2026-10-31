#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import os
  import Testing

  @testable import SoyehtClient

  /// Plays the `soyeht` program: reports are pending until acknowledged, as the real one does.
  private final class FakeProgram: ServiceCommandRunning, Sendable {
    private struct State {
      var pending: [String]
      var calls: [[String]] = []
      var failDiagnostics = false
      var failImport = false
    }
    private let state: OSAllocatedUnfairLock<State>

    init(pending: Int, failDiagnostics: Bool = false, failImport: Bool = false) {
      state = OSAllocatedUnfairLock(
        initialState: State(
          pending: (0..<pending).map { "report-\($0)" }, failDiagnostics: failDiagnostics,
          failImport: failImport))
    }

    /// A report that arrives on its own, as when a program crashes while the notice is up.
    func arrive(_ name: String) { state.withLock { $0.pending.append(name) } }

    var calls: [[String]] { state.withLock { $0.calls } }

    func run(_ executable: URL, arguments: [String]) async throws -> Data {
      try state.withLock { state in
        state.calls.append(arguments)
        let action = Array(arguments.dropFirst(4))
        switch action {
        case ["crash-reports", "status"]:
          let names = state.pending.map { "\"\($0)\"" }.joined(separator: ",")
          return Data(
            #"{"pending":\#(state.pending.count),"reports":[\#(names)],"latest":null}"#.utf8)
        case _ where action.starts(with: ["crash-reports", "acknowledge"]):
          let named = Array(action.dropFirst(2))
          let before = state.pending.count
          state.pending.removeAll { named.isEmpty || named.contains($0) }
          return Data(#"{"acknowledged":\#(before - state.pending.count)}"#.utf8)
        case ["diagnostics"]:
          if state.failDiagnostics { throw ServiceCommandError.timedOut }
          return Data("## Soyeht diagnostics\n### Crash reports\n".utf8)
        default:
          if action.first == "crash-reports", action.dropFirst().first == "import" {
            if state.failImport { throw ServiceCommandError.failed(status: 1, detail: "refused") }
            state.pending.append("imported-\(state.pending.count)")
            return Data(#"{"imported":"now"}"#.utf8)
          }
          throw ServiceCommandError.failed(status: 2, detail: "unknown")
        }
      }
    }
  }

  private final class Clipboard: @unchecked Sendable { var written: [String] = [] }

  private let bundled = URL(fileURLWithPath: "/Applications/Soyeht Next.app/Contents/MacOS/soyeht")

  private func data(_ name: String) -> URL {
    FileManager.default.temporaryDirectory.appending(
      path: "crash-notice-\(name)-\(UUID().uuidString)", directoryHint: .isDirectory)
  }

  @MainActor private func notice(_ program: FakeProgram, _ directory: URL) -> CrashNotice {
    CrashNotice(
      command: BundledEngineService(
        command: bundled, profile: .prod, dataDirectory: directory, runner: program),
      dataDirectory: directory)
  }

  @MainActor private func diagnostics(
    _ program: FakeProgram, _ clipboard: Clipboard, _ directory: URL
  )
    -> DiagnosticsCopy
  {
    DiagnosticsCopy(
      command: BundledEngineService(
        command: bundled, profile: .prod, dataDirectory: directory, runner: program),
      write: { clipboard.written.append($0) })
  }

  @MainActor @Test func aPendingReportShowsTheNoticeAndNoneHidesIt() async {
    let directory = data("status")
    let shown = notice(FakeProgram(pending: 1), directory)
    await shown.refresh()
    #expect(shown.isShowing)
    let quiet = notice(FakeProgram(pending: 0), directory)
    await quiet.refresh()
    #expect(!quiet.isShowing)
  }

  @MainActor @Test func theNoticeAppearsOnceDismissIsRemembered() async {
    let directory = data("once")
    let program = FakeProgram(pending: 1)
    let first = notice(program, directory)
    await first.refresh()
    #expect(first.isShowing)
    await first.dismiss()
    #expect(!first.isShowing)
    #expect(program.calls.last?.suffix(3) == ["crash-reports", "acknowledge", "report-0"])
    // The next start of the app asks again and the program says nothing is pending.
    let next = notice(program, directory)
    await next.refresh()
    #expect(!next.isShowing)
  }

  /// A notice that shows one pending report, and the action that copies the diagnostics.
  @MainActor private func shownNotice(failDiagnostics: Bool) async
    -> (CrashNotice, DiagnosticsCopy, FakeProgram, Clipboard)
  {
    let directory = data("copy")
    let program = FakeProgram(pending: 1, failDiagnostics: failDiagnostics)
    let clipboard = Clipboard()
    let shown = notice(program, directory)
    await shown.refresh()
    return (shown, diagnostics(program, clipboard, directory), program, clipboard)
  }

  @MainActor @Test func copyReportCopiesTheDiagnosticsThenAcknowledges() async {
    let (shown, copy, program, clipboard) = await shownNotice(failDiagnostics: false)
    await shown.copyReport(using: copy)
    #expect(clipboard.written == ["## Soyeht diagnostics\n### Crash reports\n"])
    #expect(!shown.isShowing)
    #expect(program.calls.last?.suffix(3) == ["crash-reports", "acknowledge", "report-0"])
  }

  @MainActor @Test func aFailedCopyKeepsTheNoticeAndAcknowledgesNothing() async {
    let (shown, copy, program, clipboard) = await shownNotice(failDiagnostics: true)
    await shown.copyReport(using: copy)
    #expect(clipboard.written.isEmpty)
    #expect(shown.isShowing)
    #expect(!program.calls.contains { $0.contains("acknowledge") })
  }

  @MainActor @Test func aSystemCrashDiagnosticIsHandedToTheProgramAndRaisesTheNotice() async throws
  {
    let directory = data("import")
    defer { try? FileManager.default.removeItem(at: directory) }
    let program = FakeProgram(pending: 0)
    let shown = notice(program, directory)
    await shown.receive([Data(#"{"diagnosticMetaData":{"signal":11}}"#.utf8)])
    let importCall = try #require(program.calls.first { $0.contains("import") })
    let file = URL(fileURLWithPath: try #require(importCall.last))
    #expect(file.deletingLastPathComponent().lastPathComponent == "crashes")
    #expect(file.path.hasPrefix(directory.path))
    #expect(try Data(contentsOf: file) == Data(#"{"diagnosticMetaData":{"signal":11}}"#.utf8))
    let mode =
      try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
    #expect(mode == 0o600)
    #expect(shown.isShowing)
  }

  @MainActor @Test func dismissAcknowledgesWhatWasShownNotWhatArrivedSince() async {
    let directory = data("shown")
    let program = FakeProgram(pending: 1)
    let shown = notice(program, directory)
    await shown.refresh()
    program.arrive("a-second-crash")
    await shown.dismiss()
    let next = notice(program, directory)
    await next.refresh()
    #expect(next.isShowing, "the crash that came while the notice was up is not hidden by Dismiss")
  }

  @MainActor @Test func aDiagnosticTheProgramRefusedIsDeletedNotKept() async throws {
    let directory = data("refused")
    defer { try? FileManager.default.removeItem(at: directory) }
    let program = FakeProgram(pending: 0, failImport: true)
    let shown = notice(program, directory)
    await shown.receive([Data(#"{"diagnosticMetaData":{},"terminationReason":"free text"}"#.utf8)])
    let folder = directory.appending(path: "crashes", directoryHint: .isDirectory)
    #expect(try FileManager.default.contentsOfDirectory(atPath: folder.path).isEmpty)
    #expect(!shown.isShowing)
  }

  @MainActor @Test func staleIncomingDiagnosticsAreSweptAtStartButNothingElse() throws {
    let directory = data("sweep")
    defer { try? FileManager.default.removeItem(at: directory) }
    let folder = directory.appending(path: "crashes", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    for name in ["incoming-1.json", "incoming-2.json", "crash-000000000001-mac-0-00.txt"] {
      try Data("x".utf8).write(to: folder.appending(path: name))
    }
    CrashNotice.sweepIncoming(in: directory)
    #expect(
      try FileManager.default.contentsOfDirectory(atPath: folder.path) == [
        "crash-000000000001-mac-0-00.txt"
      ])
    // A data directory with no crashes folder is nothing to sweep.
    CrashNotice.sweepIncoming(in: data("none-at-all"))
  }

  @MainActor @Test func aCopyOfTheAppWithNoProgramShowsNothing() async {
    let none = CrashNotice(command: nil, dataDirectory: data("none"))
    await none.refresh()
    await none.dismiss()
    #expect(!none.isShowing)
  }
#endif
