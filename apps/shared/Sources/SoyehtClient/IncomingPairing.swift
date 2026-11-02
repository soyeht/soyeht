import Foundation

/// A pairing link that reached the app from outside it (a tapped link) and waits for the person's
/// answer. The camera and the paste field are the person's own act and pair at once; a link
/// another app or a web page opens is not, and pairing replaces the Mac this device controls, so
/// it asks first and names where it would connect (ADR 0129).
public struct IncomingPairing: Equatable, Identifiable, Sendable {
  /// The link, as it will be given to `PairingSession`.
  public let link: String
  /// `host:port` the device would dial.
  public let address: String
  /// This device is paired with a Mac already and would forget it.
  public let replacesMac: Bool

  public var id: String { link }

  public init(_ text: String, replacingMac: Bool) throws {
    let parsed = try PairingLink(text)
    let host = parsed.address.host ?? ""
    link = text
    address = parsed.address.port.map { "\(host):\($0)" } ?? host
    replacesMac = replacingMac
  }

  public var title: String { "Pair with the Mac at \(address)?" }

  public var message: String {
    let consequence =
      replacesMac
      ? "This device will stop controlling the Mac it is paired with now. "
      : ""
    return consequence
      + "A paired device controls that Mac: it types in terminals and drives agents. "
      + "Only continue if you opened this link on purpose."
  }
}
