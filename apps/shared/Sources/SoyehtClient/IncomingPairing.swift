import Foundation

/// A pairing link that reached the app from outside it (a tapped link) and waits for the person's
/// answer. The camera and the paste field are the person's own act and pair at once; a link
/// another app or a web page opens is not, and pairing replaces the Mac this device controls, so
/// it asks first and names where it would connect and which Mac it would stop controlling
/// (ADR 0129).
public struct IncomingPairing: Equatable, Identifiable, Sendable {
  /// The link, as it will be given to `PairingSession`.
  public let link: String
  /// `host:port` the device would dial, with an IPv6 host in brackets.
  public let address: String
  /// The Mac this device is paired with now, which pairing would forget, if there is one.
  public let replaced: Replaced?

  /// The Mac a pairing would replace.
  public struct Replaced: Equatable, Sendable {
    public let address: String
    public let engineId: String
    /// The link is for the engine this device is paired with already.
    public let sameEngine: Bool
  }

  public var id: String { link }

  public init(_ text: String, replacing current: DeviceCredential?) throws {
    let parsed = try PairingLink(text)
    link = text
    address = Self.hostAndPort(parsed.address)
    replaced = current.map {
      Replaced(
        address: Self.hostAndPort($0.address), engineId: $0.engineId,
        sameEngine: $0.engineId.caseInsensitiveCompare(parsed.engineId) == .orderedSame)
    }
  }

  /// `host:port` of a URL; an IPv6 host is written in brackets so the port can be told apart.
  static func hostAndPort(_ url: URL) -> String {
    let host = url.host ?? ""
    let written = host.contains(":") ? "[\(host)]" : host
    return url.port.map { "\(written):\($0)" } ?? written
  }

  public var title: String {
    replaced?.sameEngine == true
      ? "Pair again with the Mac at \(address)?" : "Pair with the Mac at \(address)?"
  }

  public var message: String {
    let consequence: String
    if let replaced {
      let known = "the Mac at \(replaced.address) (engine \(replaced.engineId.prefix(8)))"
      consequence =
        replaced.sameEngine
        ? "This is \(known) that this device is paired with already; pairing again replaces its saved access. "
        : "This device will stop controlling \(known). "
    } else {
      consequence = ""
    }
    return consequence
      + "A paired device controls that Mac: it types in terminals and drives agents. "
      + "Only continue if you opened this link on purpose."
  }
}
