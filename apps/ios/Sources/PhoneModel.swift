import Observation
import SoyehtClient
import UIKit

@MainActor @Observable
final class PhoneModel {
  var client: EngineClient?
  var pairing: PairingSession?
  var problem: String?
  @ObservationIgnored private var pairTask: Task<Void, Never>?
  @ObservationIgnored private var loaded = false

  func resume() {
    if !loaded {
      do {
        if let credential = try CredentialVault.load() {
          client = EngineClient(credential: credential)
        }
        loaded = true
      } catch { problem = error.localizedDescription }
    }
    client?.start()
  }
  func pair(_ uri: String) {
    guard pairTask == nil else { return }
    do { _ = try PairingLink(uri) } catch {
      problem = error.localizedDescription
      return
    }
    let pairing = PairingSession()
    self.pairing = pairing
    pairTask = Task {
      defer { pairTask = nil }
      do {
        let credential = try await pairing.pair(uri, deviceName: UIDevice.current.name)
        try CredentialVault.save(credential)
        client?.stop()
        client = EngineClient(credential: credential)
        self.pairing = nil
        client?.start()
      } catch is CancellationError { self.pairing = nil } catch {
        self.pairing = nil
        problem = error.localizedDescription
      }
    }
  }
  func cancelPairing() {
    pairTask?.cancel()
    pairing?.cancel()
    pairing = nil
  }
  func forgetMac() {
    do {
      try CredentialVault.forget()
      client?.stop()
      client = nil
    } catch { problem = error.localizedDescription }
  }
}
