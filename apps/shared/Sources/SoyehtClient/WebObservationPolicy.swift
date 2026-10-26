import Foundation

/// Where the app may watch a page from inside its own world.
///
/// Console and network capture work by wrapping the page's `console`, `fetch` and
/// `XMLHttpRequest`, which a page can see (ADR 0109). That is acceptable on the person's own
/// development servers and never on a site that belongs to someone else, so the wrapper is
/// installed for a navigation only when this policy says the target is local.
public enum WebObservationPolicy {
  public static func observes(_ url: URL?) -> Bool {
    guard let url, let scheme = url.scheme?.lowercased(), ["http", "https"].contains(scheme),
      let host = url.host?.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
    else { return false }
    if host == "localhost" || host == "::1" || host.hasSuffix(".localhost")
      || host.hasSuffix(".local")
    {
      return true
    }
    return isPrivateIPv4(host)
  }

  private static func isPrivateIPv4(_ host: String) -> Bool {
    let parts = host.split(separator: ".", omittingEmptySubsequences: false)
    guard parts.count == 4 else { return false }
    // WebKit reads a leading zero as octal (`010.0.0.1` is 8.0.0.1), so it is never a private form.
    guard parts.allSatisfy({ $0.count == 1 || !$0.hasPrefix("0") }) else { return false }
    let octets = parts.compactMap { UInt8($0) }
    guard octets.count == 4 else { return false }
    switch (octets[0], octets[1]) {
    case (127, _), (10, _), (192, 168), (169, 254): return true
    case (172, 16...31): return true
    default: return false
    }
  }
}
