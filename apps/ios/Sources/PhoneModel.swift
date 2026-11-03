import Observation
import SoyehtClient
import UIKit

@MainActor @Observable
final class PhoneModel {
  var client: EngineClient?
  /// The access to the Mac this device is paired with, as the Keychain holds it.
  private(set) var credential: DeviceCredential?
  var pairing: PairingSession?
  var problem: String?
  /// A tapped pairing link that waits for the person's answer.
  var incomingPairing: IncomingPairing?
  /// The scanner or the settings are on screen.
  var sheetPresented = false
  /// A pane a tapped notification asked to show; cleared once it is on screen.
  var requestedPaneId: String?
  @ObservationIgnored let push = PhonePush()
  @ObservationIgnored private var pairTask: Task<Void, Never>?
  @ObservationIgnored private var loaded = false

  func resume() {
    if !loaded {
      do {
        if let saved = try CredentialVault.load() {
          credential = saved
          client = EngineClient(credential: saved)
        }
        loaded = true
      } catch { problem = error.localizedDescription }
    }
    client?.start()
  }
  /// A link another app or a web page opened: the person is asked before it pairs (ADR 0129).
  /// The camera and the paste field pair at once through `pair`.
  /// A link that arrives while another question, a pairing or a sheet is up is ignored: it must not
  /// change what the person is deciding.
  func receive(link: String) {
    guard pairTask == nil, incomingPairing == nil, !sheetPresented else { return }
    do {
      incomingPairing = try IncomingPairing(link, replacing: credential)
    } catch { problem = error.localizedDescription }
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
        let paired = try await pairing.pair(uri, deviceName: UIDevice.current.name)
        try CredentialVault.save(paired)
        client?.stop()
        credential = paired
        client = EngineClient(credential: paired)
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
      credential = nil
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
