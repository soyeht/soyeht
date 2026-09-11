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
    reader.stop()
    #expect(reader.panes.isEmpty)
    #expect(screen.isEmpty)
    local.createPane(directory: data)
    try await phoneWait { local.panes.count == 2 }
    reader.start()
    try await phoneWait { reader.connected && reader.panes.count == 2 }
    local.revokeDevice(credential.deviceId)
    try await phoneWait { reader.credentialRejected && local.devices.isEmpty }
    #expect(!reader.connected)
    #expect(reader.panes.isEmpty)
  }

  @MainActor private func phoneWait(_ condition: () -> Bool) async throws {
    for _ in 0..<300 {
      if condition() { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw ClientFailure.timeout
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
