#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import os
  import Testing

  @testable import SoyehtClient

  /// Plays the `soyeht` program: reports are pending until acknowledged, as the real one does.
  private final class FakeProgram: ServiceCommandRunning, Sendable {
    private struct State {
      var pending: Int
      var calls: [[String]] = []
      var failDiagnostics = false
    }
    private let state: OSAllocatedUnfairLock<State>

    init(pending: Int, failDiagnostics: Bool = false) {
      state = OSAllocatedUnfairLock(
        initialState: State(pending: pending, failDiagnostics: failDiagnostics))
    }

    var calls: [[String]] { state.withLock { $0.calls } }

    func run(_ executable: URL, arguments: [String]) async throws -> Data {
      try state.withLock { state in
        state.calls.append(arguments)
        let action = Array(arguments.dropFirst(4))
        switch action {
        case ["crash-reports", "status"]:
          return Data(#"{"pending":\#(state.pending),"latest":null}"#.utf8)
        case ["crash-reports", "acknowledge"]:
          let count = state.pending
          state.pending = 0
          return Data(#"{"acknowledged":\#(count)}"#.utf8)
        case ["diagnostics"]:
          if state.failDiagnostics { throw ServiceCommandError.timedOut }
          return Data("## Soyeht diagnostics\n### Crash reports\n".utf8)
        default:
          if action.first == "crash-reports", action.dropFirst().first == "import" {
            state.pending += 1
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
    #expect(program.calls.last?.suffix(2) == ["crash-reports", "acknowledge"])
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
    #expect(program.calls.last?.suffix(2) == ["crash-reports", "acknowledge"])
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

  @MainActor @Test func aCopyOfTheAppWithNoProgramShowsNothing() async {
    let none = CrashNotice(command: nil, dataDirectory: data("none"))
    await none.refresh()
    await none.dismiss()
    #expect(!none.isShowing)
  }
#endif
