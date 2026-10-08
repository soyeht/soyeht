import Observation
import SoyehtClient
import UIKit

@MainActor @Observable
final class PhoneModel {
  var client: EngineClient?
  var pairing: PairingSession?
  var problem: String?
  /// A pane a tapped notification asked to show; cleared once it is on screen.
  var requestedPaneId: String?
  @ObservationIgnored let push = PhonePush()
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
    do {
      let reachability = try PairingLink(uri).reachability
      #if targetEnvironment(simulator)
        _ = reachability  // The simulator shares the Mac's loopback.
      #else
        // On a device a loopback link names the phone itself: say why it cannot connect.
        if let text = reachability.problemText {
          problem = text
          return
        }
      #endif
    } catch {
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
  /// Answers from a notification action through the same request the pane uses. When the
  /// Mac cannot be reached, the alert comes back so the person can try again.
  func answerFromNotification(
    paneId: String, requestId: String, optionId: String, retry: PhoneNotice
  ) async {
    guard let client else { return await retry.postUndelivered() }
    do {
      try await client.answerPermission(paneId: paneId, requestId: requestId, optionId: optionId)
    } catch {
      await retry.postUndelivered()
    }
    if UIApplication.shared.applicationState == .background { client.stop() }
  }
  func forgetMac() {
    do {
      try CredentialVault.forget()
      // Best effort, silent by design: a Mac this device no longer knows should stop
      // sending alerts, and the forgotten Mac cannot be retried from here.
      if let forgotten = client {
        Task {
          if forgotten.connected { try? await forgotten.unregisterPush() }
          forgotten.stop()
        }
      }
      client = nil
    } catch { problem = error.localizedDescription }
  }
}
