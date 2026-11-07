import Foundation
import SoyehtProtocol

public struct PairPrompt: Identifiable, Sendable {
  public let requestId: String
  public let deviceName: String
  public let fingerprint: String
  public let expiresAt: String?
  public var id: String { requestId }
  /// The short code to compare with the one the new device shows (ADR 0152).
  public var comparisonCode: String { PairingCopy.comparisonCode(fingerprint) ?? fingerprint }

  init(_ event: DevicePairRequestedEvent) {
    requestId = event.requestId
    deviceName = event.deviceName
    fingerprint = event.fingerprint
    expiresAt = nil
  }
  init(_ pending: DevicePairPending) {
    requestId = pending.requestId
    deviceName = pending.deviceName
    fingerprint = pending.fingerprint
    expiresAt = pending.expiresAt
  }
}
