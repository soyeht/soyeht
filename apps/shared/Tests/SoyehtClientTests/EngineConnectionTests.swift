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
    let client = EngineClient(socket: directory.appendingPathComponent("engine.sock").path)
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
    let source = directory.appendingPathComponent("sample.swift")
    try "let value = 1\n".write(to: source, atomically: true, encoding: .utf8)
    let workspaceId = try #require(client.selectedPane?.workspaceId)
    let opened = try await client.openEditorFile(workspaceId: workspaceId, path: "sample.swift")
    #expect(opened.viewport.lines.first?.text == "let value = 1")
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
    client.closePane(opened.pane.paneId)
    try await until { !client.panes.contains { $0.paneId == opened.pane.paneId } }
    client.closePane(paneId)
    try await until { client.panes.isEmpty }
  }

  @MainActor private func until(_ condition: () -> Bool) async throws {
    for _ in 0..<300 {
      if condition() { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw ClientFailure.timeout
  }
#endif
