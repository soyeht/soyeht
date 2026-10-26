import Foundation

/// How the Mac web pane names itself to a site (ADR 0109).
///
/// The pane is the system's WebKit, the engine Safari uses. A bare web view leaves out the
/// `Version/` and `Safari/` tokens, and sites such as Google sign-in refuse what looks like an
/// embedded browser. The pane therefore appends the tokens of the Safari installed on this Mac,
/// and claims no other browser.
public enum SafariIdentity {
  /// The token Safari itself puts after `AppleWebKit/605.1.15 (KHTML, like Gecko)`.
  public static func applicationName(installedVersion: String?, osMajorVersion: Int) -> String {
    "Version/\(version(installedVersion, osMajorVersion: osMajorVersion)) Safari/605.1.15"
  }

  /// The version Safari.app reports, when it is a plain dotted number; else the major version
  /// that ships with the system (Safari 16, 17 and 18 shipped with macOS 13, 14 and 15; since
  /// macOS 26 both carry the same number).
  static func version(_ installed: String?, osMajorVersion: Int) -> String {
    if let installed, !installed.isEmpty,
      installed.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == ".") }),
      !installed.hasPrefix("."), !installed.hasSuffix("."), !installed.contains("..")
    {
      return installed
    }
    return "\(osMajorVersion >= 26 ? osMajorVersion : osMajorVersion + 3).0"
  }
}

#if os(macOS)
  import AppKit

  extension SafariIdentity {
    /// The token for this Mac, from the installed Safari's own bundle at run time.
    public static var applicationName: String {
      let safari = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Safari")
      let installed =
        safari.flatMap { Bundle(url: $0) }?
        .object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
      return applicationName(
        installedVersion: installed,
        osMajorVersion: ProcessInfo.processInfo.operatingSystemVersion.majorVersion)
    }
  }
#endif
