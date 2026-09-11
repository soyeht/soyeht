#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing
  @testable import SoyehtClient

  @MainActor @Test func phonePairsByConsentReadsFreshStateAndLosesRevokedAccess() async throws {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .deletingLastPathComponent()
    let bins =
      ProcessInfo.processInfo.environment["SOYEHT_TEST_BIN_DIR"].map { URL(fileURLWithPath: $0) }
      ?? root.appendingPathComponent("engine/target/debug")
    let data = URL(fileURLWithPath: "/tmp/sn-phone-" + UUID().uuidString.prefix(8))
    try FileManager.default.createDirectory(
      at: data, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    try "PROMPT='PHONE_READY> '\nHISTFILE=\nSAVEHIST=0\n".write(
      to: data.appendingPathComponent(".zshrc"), atomically: true, encoding: .utf8)
    let fake = bins.appendingPathComponent("soyeht-fake-acp")
    let registry: [[String: Any]] = [
      [
        "id": "swift-test",
        "name": "Swift test agent",
        "command": fake.path,
        "args": [
          "--state-dir", data.appendingPathComponent("native-agent").path,
          "--load", "supported",
        ],
        "version": "1",
      ]
    ]
    try JSONSerialization.data(withJSONObject: registry).write(
      to: data.appendingPathComponent("harnesses.json"), options: .atomic)
    let log = data.appendingPathComponent("test.log")
    FileManager.default.createFile(atPath: log.path, contents: nil)
    let output = try FileHandle(forWritingTo: log)
    func launch(_ name: String) throws -> Process {
      let process = Process()
      process.executableURL = bins.appendingPathComponent(name)
      process.arguments = ["--data-dir", data.path]
      if name == "soyeht-engine" { process.arguments! += ["--listen", "127.0.0.1:0"] }
      process.environment = ProcessInfo.processInfo.environment
      process.environment?["ZDOTDIR"] = data.path
      process.standardOutput = output
      process.standardError = output
      try process.run()
      return process
    }
    let supervisor = try launch("soyeht-ptyd")
    var engine: Process?
    let local = EngineClient(socket: data.appendingPathComponent("engine.sock").path)
    var phone: EngineClient?
    defer {
      phone?.stop()
      for pane in local.panes {
        let close = Process()
        close.executableURL = bins.appendingPathComponent("soyeht")
        close.arguments = [
          "--profile", "dev", "--data-dir", data.path, "pane", "close", pane.paneId,
        ]
        close.standardOutput = output
        close.standardError = output
        if (try? close.run()) != nil { close.waitUntilExit() }
      }
      local.stop()
      engine?.terminate()
      supervisor.terminate()
      if engine?.isRunning == true { engine?.waitUntilExit() }
      if supervisor.isRunning { supervisor.waitUntilExit() }
      try? output.close()
      try? FileManager.default.removeItem(at: data)
    }
    try await phoneWait {
      FileManager.default.fileExists(atPath: data.appendingPathComponent("ptyd.sock").path)
    }
    engine = try launch("soyeht-engine")
    local.start()
    try await phoneWait { local.connected }
    local.createPane(directory: data)
    try await phoneWait { local.selectedPane?.pid != nil }
    local.beginPairing()
    try await phoneWait { local.pairCode != nil }
    let pairing = PairingSession()
    let uri = try #require(local.pairCode?.uri)
    let pair = Task { try await pairing.pair(uri, deviceName: "Test iPhone") }
    defer {
      pair.cancel()
      pairing.cancel()
    }
    try await phoneWait { local.pairRequests.count == 1 }
    let request = try #require(local.pairRequests.first)
    #expect(request.fingerprint == pairing.fingerprint)
    #expect(local.devices.isEmpty)
    local.decidePairing(request, approve: true)
    let credential = try await pair.value
    try await phoneWait { local.devices.count == 1 && local.pairRequests.isEmpty }
    let roundTrip = try JSONDecoder().decode(
      DeviceCredential.self, from: JSONEncoder().encode(credential))
    let reader = EngineClient(credential: roundTrip)
    phone = reader
    var screen = Data()
    reader.terminalOutput = { bytes, reset in
      if reset { screen.removeAll() }
      screen.append(bytes)
    }
    reader.start()
    try await phoneWait { reader.connected && !reader.panes.isEmpty }
    #expect(reader.readOnly)
    #expect(reader.selectedPane?.pid == local.selectedPane?.pid)
    try await phoneWait { String(decoding: screen, as: UTF8.self).contains("PHONE_READY>") }
    local.input(Data("printf 'PHONE_READER_%s\\n' OK\r".utf8))
    try await phoneWait { String(decoding: screen, as: UTF8.self).contains("PHONE_READER_OK") }

    local.createAgentPane(
      directory: data, harness: "swift-test", model: nil, effort: nil, permission: .ask)
    try await phoneWait("local agent session did not open") {
      local.panes.contains { $0.mode == .agent && $0.agent?.sessionId != nil }
    }
    let agentId = try #require(local.panes.first { $0.mode == .agent }?.paneId)
    try await phoneWait("remote client did not receive the agent pane") {
      reader.panes.contains { $0.paneId == agentId }
    }
    reader.select(agentId)
    reader.sendPrompt(agentId, text: "hello from phone")
    try await phoneWait("two Swift clients did not receive the completed reply") {
      [local, reader].allSatisfy { replica in
        replica.agentPanes[agentId]?.messages.contains {
          $0.role == .assistant && $0.complete && $0.text == "Echo: hello from phone"
        } == true
      }
    }
    try await phoneWait("two Swift clients did not converge on the same transcript") {
      let localBytes = try? JSONEncoder().encode(local.agentPanes[agentId]?.messages ?? [])
      let remoteBytes = try? JSONEncoder().encode(reader.agentPanes[agentId]?.messages ?? [])
      return localBytes == remoteBytes
        && local.agentPanes[agentId]?.messages.contains {
          $0.role == .user && $0.text == "hello from phone"
        } == true
    }
    let localTranscript = try JSONEncoder().encode(local.agentPanes[agentId]?.messages ?? [])
    let remoteTranscript = try JSONEncoder().encode(reader.agentPanes[agentId]?.messages ?? [])
    #expect(localTranscript == remoteTranscript)

    local.sendPrompt(agentId, text: "permission")
    try await phoneWait("permission did not reach both Swift clients") {
      local.agentPanes[agentId]?.permission != nil
        && reader.agentPanes[agentId]?.permission != nil
    }
    let permission = try #require(reader.agentPanes[agentId]?.permission)
    #expect(local.agentPanes[agentId]?.permission?.requestId == permission.requestId)
    reader.answerPermission(permission, optionId: "once")
    try await phoneWait("remote permission answer did not settle") {
      local.agentPanes[agentId]?.permission == nil
        && reader.agentPanes[agentId]?.permission == nil
        && local.panes.first(where: { $0.paneId == agentId })?.state == .idle
    }
    reader.setYolo(agentId, on: true)
    try await phoneWait("yolo enablement did not reach both Swift clients") {
      [local, reader].allSatisfy {
        $0.panes.first(where: { $0.paneId == agentId })?.agent?.yolo == true
      }
    }
    reader.setYolo(agentId, on: false)
    try await phoneWait("yolo disablement did not reach both Swift clients") {
      [local, reader].allSatisfy {
        $0.panes.first(where: { $0.paneId == agentId })?.agent?.yolo == false
      }
    }

    reader.stop()
    #expect(reader.panes.isEmpty)
    #expect(screen.isEmpty)
    let paneCount = local.panes.count
    local.createPane(directory: data)
    try await phoneWait("local client did not receive the added pane") {
      local.panes.count == paneCount + 1
    }
    reader.start()
    try await phoneWait(
      "remote snapshot did not include every pane",
      details:
        "connected=\(reader.connected), panes=\(reader.panes.count), expected=\(paneCount + 1), state=\(reader.connectionText)"
    ) {
      reader.connected && reader.panes.count == paneCount + 1
    }
    local.revokeDevice(credential.deviceId)
    try await phoneWait("revocation did not close the remote client") {
      reader.credentialRejected && local.devices.isEmpty
    }
    #expect(!reader.connected)
    #expect(reader.panes.isEmpty)
  }

  @MainActor private func phoneWait(
    _ message: String = "condition did not become true", details: @autoclosure () -> String = "",
    _ condition: () -> Bool
  ) async throws {
    for _ in 0..<300 {
      if condition() { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    let detail = details()
    throw ClientFailure.remote("test_timeout", detail.isEmpty ? message : "\(message): \(detail)")
  }

  @Test func pairingLinkRejectsExternalTargetsAndDuplicateFields() throws {
    let prefix = "soyeht-next://pair?engineId=11111111-1111-4111-8111-111111111111&address="
    let code = String(repeating: "a", count: 43)
    #expect(try PairingLink(prefix + "100.64.0.1:8291&code=" + code).address.host == "100.64.0.1")
    #expect(try PairingLink(prefix + "[fd7a:115c:a1e0::1]:8291&code=" + code).address.port == 8291)
    for address in [
      "example.com:8291", "192.168.1.2:8291", "0.0.0.0:8291", "100.64.0.1:8291/extra",
      "user@100.64.0.1:8291",
    ] {
      #expect(throws: ClientFailure.self) { try PairingLink(prefix + address + "&code=" + code) }
    }
    #expect(throws: ClientFailure.self) {
      try PairingLink(prefix + "100.64.0.1:8291&code=" + code + "&code=" + code)
    }
  }
#endif
