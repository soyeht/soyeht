#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing
  @testable import SoyehtClient

  private func preparePhoneFixture() throws -> (bins: URL, data: URL, output: FileHandle) {
    let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .deletingLastPathComponent()
    let bins =
      ProcessInfo.processInfo.environment["SOYEHT_TEST_BIN_DIR"].map { URL(fileURLWithPath: $0) }
      ?? root.appendingPathComponent("engine/target/debug")
    // The engine refuses to start an agent without `soyeht-mcp` next to it, and the pane then
    // waits for a session that never opens. Name the missing program before the 30 s wait.
    let missing = ["soyeht-engine", "soyeht-ptyd", "soyeht", "soyeht-fake-acp", "soyeht-mcp"]
      .filter {
        !FileManager.default.isExecutableFile(atPath: bins.appendingPathComponent($0).path)
      }
    guard missing.isEmpty else {
      throw ClientFailure.remote(
        "test_setup",
        "missing \(missing.joined(separator: ", ")) in \(bins.path); build the whole workspace")
    }
    let data = URL(fileURLWithPath: "/tmp/sn-phone-" + UUID().uuidString.prefix(8))
    try FileManager.default.createDirectory(
      at: data, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
    try "PROMPT='PHONE_READY> '\nHISTFILE=\nSAVEHIST=0\n".write(
      to: data.appendingPathComponent(".zshrc"), atomically: true, encoding: .utf8)
    let registry: [[String: Any]] = [
      [
        "id": "swift-test",
        "name": "Swift test agent",
        "command": bins.appendingPathComponent("soyeht-fake-acp").path,
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
    return (bins, data, try FileHandle(forWritingTo: log))
  }

  private func launchPhoneProcess(
    _ name: String, bins: URL, data: URL, output: FileHandle
  ) throws -> Process {
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

  @MainActor private func cleanPhoneFixture(
    phone: EngineClient?, local: EngineClient, engine: Process?, supervisor: Process,
    bins: URL, data: URL, output: FileHandle
  ) {
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

  @MainActor private func pairCredential(_ local: EngineClient) async throws -> DeviceCredential {
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
    return try JSONDecoder().decode(
      DeviceCredential.self, from: JSONEncoder().encode(credential))
  }

  @MainActor private func selectedAgentValue(
    _ client: EngineClient, pane: String, option: String
  ) -> String? {
    guard
      let value = client.panes.first(where: { $0.paneId == pane })?.agent?.options
        .first(where: { $0.id == option })?.currentValue,
      case .v0(let selected) = value
    else { return nil }
    return selected
  }

  @MainActor private func exerciseAgentPane(
    local: EngineClient, reader: EngineClient, data: URL
  ) async throws {
    _ = try await local.createAgentPaneNow(
      directory: data, harness: "swift-test", model: nil, effort: nil)
    try await phoneWait(
      "local agent session did not open",
      details: local.panes.first { $0.mode == .agent }?.phrase ?? "no agent pane"
    ) {
      local.panes.contains { $0.mode == .agent && $0.agent?.sessionId != nil }
    }
    let agent = try #require(local.panes.first { $0.mode == .agent }?.paneId)
    try await phoneWait("remote client did not receive the agent pane") {
      reader.panes.contains { $0.paneId == agent }
    }
    reader.select(agent)
    reader.setAgentConfig(agent, configId: "model", value: .v0("model-b"))
    try await phoneWait("remote model selection did not converge") {
      [local, reader].allSatisfy {
        selectedAgentValue($0, pane: agent, option: "model") == "model-b"
      }
    }
    reader.setAgentConfig(agent, configId: "effort", value: .v0("high"))
    try await phoneWait("remote effort selection did not converge") {
      [local, reader].allSatisfy { selectedAgentValue($0, pane: agent, option: "effort") == "high" }
    }
    reader.sendPrompt(agent, text: "hello from phone")
    try await phoneWait("two Swift clients did not receive the completed reply") {
      [local, reader].allSatisfy { replica in
        replica.agentPanes[agent]?.messages.contains {
          $0.role == .assistant && $0.complete && $0.text == "Echo: hello from phone"
        } == true
      }
    }
    try await phoneWait("two Swift clients did not converge on the same transcript") {
      let localBytes = try? canonicalTranscript(local.agentPanes[agent]?.messages ?? [])
      let remoteBytes = try? canonicalTranscript(reader.agentPanes[agent]?.messages ?? [])
      return localBytes == remoteBytes
        && local.agentPanes[agent]?.messages.contains {
          $0.role == .user && $0.text == "hello from phone"
        } == true
    }
    #expect(
      try canonicalTranscript(local.agentPanes[agent]?.messages ?? [])
        == canonicalTranscript(reader.agentPanes[agent]?.messages ?? []))
    local.sendPrompt(agent, text: "permission")
    try await phoneWait("permission did not reach both Swift clients") {
      local.agentPanes[agent]?.permission != nil && reader.agentPanes[agent]?.permission != nil
    }
    let permission = try #require(reader.agentPanes[agent]?.permission)
    #expect(local.agentPanes[agent]?.permission?.requestId == permission.requestId)
    reader.answerPermission(permission, optionId: "once")
    try await phoneWait("remote permission answer did not settle") {
      local.agentPanes[agent]?.permission == nil
        && reader.agentPanes[agent]?.permission == nil
        && local.panes.first(where: { $0.paneId == agent })?.state == .idle
    }
    // The phone changes the harness's own mode through the same native
    // configuration request the Mac uses, and both show only confirmed values.
    for (change, expected) in [
      ({ reader.setPermissionMode(agent, to: "allow_all") }, "allow_all"),
      ({ reader.setPermissionMode(agent, to: "ask") }, "ask"),
      ({ reader.cyclePermissionMode(agent) }, "accept_edits"),
    ] {
      change()
      try await phoneWait("phone mode \(expected) did not reach both Swift clients") {
        [local, reader].allSatisfy {
          let modes = $0.permissionModes(paneId: agent)
          return modes?.current == expected && modes?.pending == nil
        }
      }
    }
    #expect(selectedAgentValue(local, pane: agent, option: "mode") == "accept_edits")
  }

  @MainActor private func reconnectAndRevoke(
    local: EngineClient, reader: EngineClient, credential: DeviceCredential, data: URL
  ) async throws {
    reader.stop()
    #expect(reader.panes.isEmpty)
    let paneCount = local.panes.count
    _ = try await local.createTerminalPane(directory: data)
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

  @MainActor @Test func phonePairsByConsentReadsFreshStateAndLosesRevokedAccess() async throws {
    let fixture = try preparePhoneFixture()
    let supervisor = try launchPhoneProcess(
      "soyeht-ptyd", bins: fixture.bins, data: fixture.data, output: fixture.output)
    var engine: Process?
    let local = EngineClient(socket: fixture.data.appendingPathComponent("engine.sock").path)
    var phone: EngineClient?
    defer {
      cleanPhoneFixture(
        phone: phone, local: local, engine: engine, supervisor: supervisor,
        bins: fixture.bins, data: fixture.data, output: fixture.output)
    }
    try await phoneWait {
      FileManager.default.fileExists(atPath: fixture.data.appendingPathComponent("ptyd.sock").path)
    }
    engine = try launchPhoneProcess(
      "soyeht-engine", bins: fixture.bins, data: fixture.data, output: fixture.output)
    local.start()
    try await phoneWait { local.connected }
    _ = try await local.createTerminalPane(directory: fixture.data)
    try await phoneWait { local.selectedPane?.pid != nil }
    let credential = try await pairCredential(local)
    let reader = EngineClient(credential: credential)
    phone = reader
    var screen = Data()
    reader.terminalOutput = { bytes, reset in
      if reset { screen.removeAll() }
      screen.append(bytes)
    }
    reader.start()
    try await phoneWait { reader.connected && !reader.panes.isEmpty }
    #expect(reader.pairedDevice)
    #expect(reader.selectedPane?.pid == local.selectedPane?.pid)
    try await phoneWait { String(decoding: screen, as: UTF8.self).contains("PHONE_READY>") }
    local.input(Data("printf 'PHONE_READER_%s\\n' OK\r".utf8))
    try await phoneWait { String(decoding: screen, as: UTF8.self).contains("PHONE_READER_OK") }
    // The paired device types through the same pane.input path as the Mac.
    reader.input(Data("printf 'PHONE_WRITER_%s\\n' OK\r".utf8))
    try await phoneWait { String(decoding: screen, as: UTF8.self).contains("PHONE_WRITER_OK") }
    try await exerciseAgentPane(local: local, reader: reader, data: fixture.data)
    try await reconnectAndRevoke(
      local: local, reader: reader, credential: credential, data: fixture.data)
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

  private func canonicalTranscript(_ messages: [TranscriptMessage]) throws -> Data {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    return try encoder.encode(messages)
  }

  @Test func pairingLinkRejectsExternalTargetsAndDuplicateFields() throws {
    let prefix = "soyeht-next://pair?engineId=11111111-1111-4111-8111-111111111111&address="
    let code = String(repeating: "a", count: 43)
    let key = "&engineKey=" + String(repeating: "A", count: 43)
    #expect(
      try PairingLink(prefix + "100.64.0.1:8291&code=" + code + key).address.host == "100.64.0.1")
    #expect(
      try PairingLink(prefix + "[fd7a:115c:a1e0::1]:8291&code=" + code + key).address.port == 8291)
    for address in [
      "example.com:8291", "192.168.1.2:8291", "0.0.0.0:8291", "100.64.0.1:8291/extra",
      "user@100.64.0.1:8291",
    ] {
      #expect(throws: ClientFailure.self) {
        try PairingLink(prefix + address + "&code=" + code + key)
      }
    }
    #expect(throws: ClientFailure.self) {
      try PairingLink(prefix + "100.64.0.1:8291&code=" + code + "&code=" + code + key)
    }
    // The engine's key is part of the link: without it, with a wrong length or twice, it is refused.
    let address = prefix + "100.64.0.1:8291&code=" + code
    #expect(throws: ClientFailure.self) { try PairingLink(address) }
    #expect(throws: ClientFailure.self) { try PairingLink(address + "&engineKey=AAAA") }
    #expect(throws: ClientFailure.self) { try PairingLink(address + key + key) }
  }
#endif
