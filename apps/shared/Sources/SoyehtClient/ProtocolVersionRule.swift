import SoyehtProtocol

/// The protocol versions are a range (ADR 0161): the engine welcomes this client at the lower of
/// the two versions, and the client speaks any version from the contract's `min` to `current`.
/// `contracts/vectors/protocol-version.json` holds the cases, which `soyeht-protocol` passes too.
extension ProtocolVersion {
  /// Whether a client serving `min` to `current` can speak the version it was welcomed at.
  static func accepts(_ welcomed: UInt32, min: UInt32, current: UInt32) -> Bool {
    (min...current).contains(welcomed)
  }

  /// [`accepts(_:min:current:)`] for this build's range.
  static func accepts(_ welcomed: UInt32) -> Bool {
    accepts(welcomed, min: Self.min, current: Self.current)
  }

  /// What the person is told when the welcome is at a version this app does not speak.
  static let updateMessage =
    "This app and the Mac speak different versions of Soyeht. Update Soyeht on the older one."
}
