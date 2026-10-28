import SoyehtProtocol

/// The `client` name a process sends in `hello`: a release build sends the plain name and a debug
/// build the `-dev` one, so the engine's log tells a person's app from a developer's. The names
/// are `x-clientPolicy.clientName`; the desktop client follows the same rule.
enum ClientName {
  static var mac: String {
    #if DEBUG
      ClientPolicy.clientNameMacDev
    #else
      ClientPolicy.clientNameMac
    #endif
  }

  static var iphone: String {
    #if DEBUG
      ClientPolicy.clientNameIphoneDev
    #else
      ClientPolicy.clientNameIphone
    #endif
  }
}

extension HelloParams {
  /// The `hello` of the Mac app's local connection.
  static func mac(capabilities: [ClientCapability]?) -> HelloParams {
    HelloParams(
      client: ClientName.mac, clientVersion: ProductVersion.current,
      protocolVersion: ProtocolVersion.current, capabilities: capabilities)
  }

  /// The `hello` of a paired iPhone, which proves it holds the device key.
  static func iphone(deviceToken: String, proof: String) -> HelloParams {
    HelloParams(
      client: ClientName.iphone, clientVersion: ProductVersion.current,
      protocolVersion: ProtocolVersion.current, deviceToken: deviceToken, proof: proof)
  }
}
