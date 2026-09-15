#if os(macOS)
  import Foundation
  import Testing
  @testable import SoyehtClient
  import SoyehtProtocol

  /// Runs against disposable daemons; never opens the installed Dev socket.
  @MainActor @Test func nativeClientReconnectsAndDrivesItsOwnShellAndEditor() async throws {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .deletingLastPathComponent()
    let binaries =
      ProcessInfo.processInfo.environment["SOYEHT_TEST_BIN_DIR"]
      .map { URL(fileURLWithPath: $0) } ?? root.appendingPathComponent("engine/target/debug")
    guard
      FileManager.default.isExecutableFile(
        atPath: binaries.appendingPathComponent("soyeht-engine").path)
    else {
      Issue.record("Build the Rust workspace before the native client integration test.")
      return
    }
    let directory = URL(fileURLWithPath: "/tmp/sn-sw-" + UUID().uuidString.prefix(8))
    try FileManager.default.createDirectory(
      at: directory, withIntermediateDirectories: true,
      attributes: [.posixPermissions: 0o700])
    try "PROMPT='SWIFT_READY> '\nHISTFILE=\nSAVEHIST=0\n".write(
      to: directory.appendingPathComponent(".zshrc"), atomically: true, encoding: .utf8)
    let log = directory.appendingPathComponent("test.log")
    FileManager.default.createFile(atPath: log.path, contents: nil)
    let output = try FileHandle(forWritingTo: log)
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
    let supervisor = try launch("soyeht-ptyd")
    var engine: Process?
    let webHost = IntegrationWebHost()
    let client = EngineClient(
      socket: directory.appendingPathComponent("engine.sock").path, webHost: webHost)
    defer {
      if engine?.isRunning == true {
        for pane in client.panes {
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
      client.stop()
      engine?.terminate()
      supervisor.terminate()
      try? output.close()
      // The supervisor only owns this fixture's children. Normal close is tested below.
      if engine?.isRunning == true { engine?.waitUntilExit() }
      if supervisor.isRunning { supervisor.waitUntilExit() }
      try? FileManager.default.removeItem(at: directory)
    }
    try await until {
      FileManager.default.fileExists(atPath: directory.appendingPathComponent("ptyd.sock").path)
    }
    engine = try launch("soyeht-engine")
    client.start()
    do { try await until { client.connected } } catch {
      Issue.record("Client connection: \(client.connectionText)")
      throw error
    }
    var screen = Data()
    client.terminalOutput = { bytes, reset in
      if reset { screen.removeAll() }
      screen.append(bytes)
    }
    client.createPane(directory: directory)
    try await until { client.selectedPane?.pid != nil }
    let paneId = try #require(client.selectedPane?.paneId)
    let pid = try #require(client.selectedPane?.pid)
    try await until { String(decoding: screen, as: UTF8.self).contains("SWIFT_READY>") }
    client.input(Data("printf 'SWIFT_CLIENT_%s\\n' READY\r".utf8))
    try await until { String(decoding: screen, as: UTF8.self).contains("SWIFT_CLIENT_READY") }
    let revisionBeforeClose = client.connectionRevision
    engine?.terminate()
    if let engine { engine.waitUntilExit() }
    try await until { !client.connected }
    #expect(client.connectionText == ClientFailure.disconnected.localizedDescription)
    engine = try launch("soyeht-engine")
    try await until { client.connected && client.selectedPane?.pid == pid }
    #expect(client.connectionRevision > revisionBeforeClose)
    try await until { String(decoding: screen, as: UTF8.self).contains("SWIFT_CLIENT_READY") }
    client.stop()
    client.start()
    try await until { client.connected && client.selectedPane?.pid == pid }
    try await exerciseEditor(client, directory: directory)
    let workspaceId = try #require(client.panes.first?.workspaceId)
    try await exerciseWritingRevision(client, workspaceId: workspaceId)
    let web = try await client.openWeb(workspaceId: workspaceId, url: "localhost:4173")
    let navigation = try await client.navigateWeb(paneId: web.paneId, action: .reload)
    #expect(navigation.accepted)
    let resized = try await client.resizeWeb(paneId: web.paneId, preset: .iphone15_pro)
    #expect(resized.viewport.width == 393)
    #expect(resized.viewport.height == 852)
    #expect(webHost.commands.map(\.command) == [.navigate, .resize])
    client.closePane(web.paneId)
    try await until { !client.panes.contains { $0.paneId == web.paneId } }
    client.closePane(paneId)
    try await until { client.panes.isEmpty }
  }

  @MainActor private final class IntegrationWebHost: WebHostPort {
    var commands: [WebCommandParams] = []

    func execute(_ command: WebCommandParams) async throws -> WebHostOutput {
      commands.append(command)
      switch command.command {
      case .navigate:
        return WebHostOutput(
          result: WebCommandResult(
            paneId: command.paneId, value: .object(["accepted": .bool(true)]),
            cookiesPresent: false))
      case .resize:
        return WebHostOutput(
          result: WebCommandResult(
            paneId: command.paneId,
            value: .object([
              "viewport": .object([
                "width": .integer(393), "height": .integer(852),
                "preset": .string("iphone_15_pro"), "zoomPercent": .integer(100),
              ])
            ]), cookiesPresent: false))
      default:
        throw ClientFailure.webHost(.internal_error, "Unexpected integration command.")
      }
    }
  }

  @MainActor private func exerciseEditor(_ client: EngineClient, directory: URL) async throws {
    let source = directory.appendingPathComponent("sample.swift")
    try "let value = 1\n".write(to: source, atomically: true, encoding: .utf8)
    let workspaceId = try #require(client.selectedPane?.workspaceId)
    let opened = try await client.openEditorFile(
      workspaceId: workspaceId, path: "sample.swift", line: 0, column: 4)
    #expect(opened.viewport.lines.first?.text == "let value = 1")
    #expect(client.selectedPaneId == opened.pane.paneId)
    #expect(client.editorNavigationTarget?.fileId == opened.file.fileId)
    #expect(client.editorNavigationTarget?.position.line == 0)
    #expect(client.editorNavigationTarget?.position.column == 4)
    _ = try await client.writeEditorFile(
      fileId: opened.file.fileId, baseRevision: opened.file.revision,
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 12),
            end: TextPosition(line: 0, column: 13)),
          text: "2")
      ])
    let edited = try await client.readEditorFile(
      fileId: opened.file.fileId, fromLine: 0, lineCount: 1)
    #expect(edited.lines.first?.text == "let value = 2")
    client.enqueueEditorEdits(
      fileId: opened.file.fileId,
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 12),
            end: TextPosition(line: 0, column: 13)),
          text: "3")
      ])
    client.enqueueEditorEdits(
      fileId: opened.file.fileId,
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 12),
            end: TextPosition(line: 0, column: 13)),
          text: "4")
      ])
    try await until { client.editorFiles[opened.file.fileId]?.file.revision == "4" }
    let queued = try await client.readEditorFile(
      fileId: opened.file.fileId, fromLine: 0, lineCount: 1)
    #expect(queued.lines.first?.text == "let value = 4")
    try await until { client.editorFiles[opened.file.fileId]?.file.dirty == false }
    let firstHistory = try await client.editorHistory(opened.file.fileId)
    let savedFour = try #require(firstHistory.items.first)
    let historical = try await client.previewEditorFile(
      fileId: opened.file.fileId, source: .history, versionId: savedFour.versionId)
    #expect(historical.lines.first?.text == "let value = 4")
    _ = try await client.writeEditorFile(
      fileId: opened.file.fileId,
      baseRevision: try #require(client.editorFiles[opened.file.fileId]?.file.revision),
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 12),
            end: TextPosition(line: 0, column: 13)),
          text: "5")
      ])
    try await until { client.editorFiles[opened.file.fileId]?.file.dirty == false }
    _ = try await client.revertEditorFile(
      opened.file.fileId, versionId: savedFour.versionId,
      baseRevision: try #require(client.editorFiles[opened.file.fileId]?.file.revision))
    try await until { client.editorFiles[opened.file.fileId]?.file.dirty == false }
    #expect(try String(contentsOf: source, encoding: .utf8) == "let value = 4\n")

    _ = try await client.writeEditorFile(
      fileId: opened.file.fileId,
      baseRevision: try #require(client.editorFiles[opened.file.fileId]?.file.revision),
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 12),
            end: TextPosition(line: 0, column: 13)),
          text: "6")
      ])
    try "let value = 7\n".write(to: source, atomically: true, encoding: .utf8)
    try await until { client.editorFiles[opened.file.fileId]?.file.conflicted == true }
    let draft = try await client.previewEditorFile(fileId: opened.file.fileId, source: .draft)
    let disk = try await client.previewEditorFile(fileId: opened.file.fileId, source: .disk)
    #expect(draft.lines.first?.text == "let value = 6")
    #expect(disk.lines.first?.text == "let value = 7")
    _ = try await client.keepEditorDraft(
      fileId: opened.file.fileId,
      baseRevision: try #require(client.editorFiles[opened.file.fileId]?.file.revision))
    try await until { client.editorFiles[opened.file.fileId]?.file.dirty == false }
    #expect(try String(contentsOf: source, encoding: .utf8) == "let value = 6\n")
    client.closePane(opened.pane.paneId)
    try await until { !client.panes.contains { $0.paneId == opened.pane.paneId } }
  }

  @MainActor private func exerciseWritingRevision(
    _ client: EngineClient, workspaceId: String
  ) async throws {
    let created = try await client.createWritingDocument(
      workspaceId: workspaceId, section: .screenplays, title: "Revision Sync")
    let fileId = created.document.file.fileId
    client.enqueueEditorEdits(
      fileId: fileId,
      edits: [
        TextEdit(
          range: TextRange(
            start: TextPosition(line: 0, column: 0),
            end: TextPosition(line: 0, column: 0)),
          text: ".INT. COFFEE SHOP - DAY\n\n@BOB")
      ])

    let beats = try await client.listWritingBeats(fileId: fileId)
    #expect(beats.beats.isEmpty)
    var stale = try #require(client.writingDocuments[fileId])
    stale.document.file.revision = created.document.file.revision
    client.writingDocuments[fileId] = stale
    #expect(try await client.listWritingBeats(fileId: fileId).beats.isEmpty)
    let structure = try await client.loadWritingStructure(fileId: fileId)
    #expect(structure.scenes.map(\.heading) == ["INT. COFFEE SHOP - DAY"])
    #expect(structure.elements.map(\.kind) == [.scene_heading, .character])
    let saved = try await client.putWritingBeat(
      fileId: fileId, title: "Opening", body: "BOB enters", category: .story, order: 0,
      sceneId: structure.scenes.first?.sceneId)
    #expect(saved.beat?.title == "Opening")
    client.closePane(created.pane.paneId)
    try await until { !client.panes.contains { $0.paneId == created.pane.paneId } }
  }

  @MainActor private func until(_ condition: () -> Bool) async throws {
    for _ in 0..<300 {
      if condition() { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw ClientFailure.timeout
  }
#endif
