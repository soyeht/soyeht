import Foundation
import SoyehtProtocol

/// Where a web pane that shows a project file as a page may go (contract 11, section 5b). The
/// rule is `soyeht_protocol::web::same_origin`; both pass `contracts/vectors/project-page-origin.json`.
/// Only `http` and `https` addresses with a plain authority (no user information, no backslash, no
/// whitespace) have an origin, so anything odd is a different one.
public enum ProjectPageOrigin {
  /// Whether `target` has the scheme, host and port of `page`.
  public static func sameOrigin(page: String, target: String) -> Bool {
    guard let page = origin(of: page), let target = origin(of: target) else { return false }
    return page == target
  }

  /// Whether a frame of the page may load `target`: its origin, or an address the page makes
  /// itself (`about:blank`, `about:srcdoc`, `data:` and `blob:`).
  public static func allowsFrame(page: String, target: String) -> Bool {
    sameOrigin(page: page, target: target) || isMadeByPage(target)
  }

  private static func isMadeByPage(_ address: String) -> Bool {
    let lowered = address.lowercased()
    return ["about:blank", "about:srcdoc"].contains(lowered)
      || lowered.hasPrefix("data:") || lowered.hasPrefix("blob:")
  }

  private struct Origin: Equatable {
    let scheme: String
    let host: String
    let port: UInt16
  }

  private static let colon = UInt8(ascii: ":")
  private static let slash = UInt8(ascii: "/")

  private static func origin(of address: String) -> Origin? {
    let rejected = address.unicodeScalars.contains {
      $0.properties.isWhitespace || $0.properties.generalCategory == .control || $0 == "\\"
    }
    if rejected { return nil }
    let bytes = Array(address.utf8)
    guard let separator = indexOf(Array("://".utf8), in: bytes) else { return nil }
    let scheme = text(bytes[..<separator]).lowercasedASCII
    let defaultPort: UInt16
    switch scheme {
    case "http": defaultPort = 80
    case "https": defaultPort = 443
    default: return nil
    }
    let rest = bytes[(separator + 3)...]
    let authority = Array(
      rest.prefix { $0 != slash && $0 != UInt8(ascii: "?") && $0 != UInt8(ascii: "#") })
    if authority.isEmpty || authority.contains(UInt8(ascii: "@")) { return nil }
    let host: String
    let portText: [UInt8]?
    if authority.first == UInt8(ascii: "[") {
      guard let close = authority.firstIndex(of: UInt8(ascii: "]")) else { return nil }
      host = text(authority[...close])
      let after = authority[(close + 1)...]
      // Whatever follows the bracket that is not a colon is read as no port, as in Rust.
      portText = after.first == colon ? Array(after.dropFirst()) : nil
    } else if let split = authority.firstIndex(of: colon) {
      host = text(authority[..<split])
      portText = Array(authority[(split + 1)...])
    } else {
      host = text(authority[...])
      portText = nil
    }
    if host.isEmpty { return nil }
    let port: UInt16
    if let portText {
      let digits = portText.allSatisfy { $0 >= UInt8(ascii: "0") && $0 <= UInt8(ascii: "9") }
      guard !portText.isEmpty, digits, let parsed = UInt16(text(portText[...])) else { return nil }
      port = parsed
    } else {
      port = defaultPort
    }
    return Origin(scheme: scheme, host: host.lowercasedASCII, port: port)
  }

  private static func text(_ bytes: ArraySlice<UInt8>) -> String {
    String(decoding: bytes, as: UTF8.self)
  }

  private static func indexOf(_ needle: [UInt8], in bytes: [UInt8]) -> Int? {
    guard bytes.count >= needle.count else { return nil }
    return (0...(bytes.count - needle.count)).first {
      bytes[$0..<($0 + needle.count)].elementsEqual(needle)
    }
  }
}

/// Whether a web pane may load an address (contract 11, section 5b). A pane that shows a project
/// file as a page is held to the origin of its own address from its first load: "no address yet"
/// means nothing may load, not "no limit".
public enum ProjectPageNavigation {
  /// Whether the pane may load `target`. `pageAddress` is the address the engine holds for the
  /// pane, and `hasTargetFrame` is false for a navigation that opens a new window. For a pane that
  /// is not a project page this is always true: the caller keeps its own scheme policy.
  public static func allows(
    isProjectPage: Bool, pageAddress: String?, target: String, isMainFrame: Bool,
    hasTargetFrame: Bool
  ) -> Bool {
    guard isProjectPage else { return true }
    guard let pageAddress, hasTargetFrame else { return false }
    return isMainFrame
      ? ProjectPageOrigin.sameOrigin(page: pageAddress, target: target)
      : ProjectPageOrigin.allowsFrame(page: pageAddress, target: target)
  }
}

/// Who a web command acts for, read from the `_soyehtAccess` object the engine adds to its args.
public enum WebCommandAccess {
  /// Whether a person asked: the command of a human session, unless it is the engine's own
  /// background reload (`background: true` on a `reload` of a `navigate`, as the reload of a
  /// project page), which selects, raises and focuses nothing (contract 11, section 7). The
  /// flag counts for nothing else: a click that carries it is still a person's act.
  public static func isPersons(_ command: WebCommandParams) -> Bool {
    guard case .object(let values) = command.args,
      case .object(let access)? = values["_soyehtAccess"],
      case .string("human")? = access["session"]
    else { return false }
    return !(isBackground(access) && isReload(command, values))
  }

  private static func isBackground(_ access: [String: JsonValue]) -> Bool {
    if case .bool(true)? = access["background"] { return true }
    return false
  }

  private static func isReload(_ command: WebCommandParams, _ args: [String: JsonValue]) -> Bool {
    guard command.command == .navigate,
      case .string(let action)? = args["action"]
    else { return false }
    return action == WebNavigationAction.reload.rawValue
  }
}

extension String {
  /// The string with only `A`-`Z` lowercased, as Rust's `to_ascii_lowercase`.
  fileprivate var lowercasedASCII: String {
    String(decoding: utf8.map { $0 >= 65 && $0 <= 90 ? $0 + 32 : $0 }, as: UTF8.self)
  }
}
