import Foundation
import SoyehtProtocol

public struct PairPrompt: Identifiable, Sendable {
  public let requestId: String
  public let deviceName: String
  public let fingerprint: String
  public let expiresAt: String?
  public var id: String { requestId }

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
