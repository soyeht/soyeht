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

extension EngineClient {
  /// Keeps `pairRequests` and `devices` in step with the engine's device events.
  func applyDeviceEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .devicePairRequested:
      let event = try params.decoded(DevicePairRequestedEvent.self)
      pairRequests.removeAll { $0.requestId == event.requestId }
      pairRequests.append(PairPrompt(event))
    case .devicePaired:
      let device = try params["device"].decoded(Device.self)
      pairRequests.removeAll { $0.fingerprint == device.fingerprint }
      devices.removeAll { $0.deviceId == device.deviceId }
      if !device.revoked { devices.append(device) }
    case .deviceRevoked: devices.removeAll { $0.deviceId == params["deviceId"].string }
    default: break
    }
  }
}
