#if os(macOS)
  import Foundation
  import Security

  /// Whether an app bundle's code signature is intact. A seam, so a test does not need a signed
  /// app.
  public protocol AppCodeValidating: Sendable {
    func isValid(_ app: URL) -> Bool
  }

  /// The system's own check of a bundle's signature (`SecStaticCodeCheckValidity`): the seal of
  /// the main executable, of every resource and of all nested code is whole, for every
  /// architecture. It does not ask who signed: the copy is the same bytes as the running app,
  /// and the check is that the copy is that app, complete, before it takes the app's place.
  public struct SystemCodeValidity: AppCodeValidating {
    public init() {}

    public func isValid(_ app: URL) -> Bool {
      var code: SecStaticCode?
      guard SecStaticCodeCreateWithPath(app as CFURL, SecCSFlags(), &code) == errSecSuccess,
        let code
      else { return false }
      let flags = SecCSFlags(
        rawValue: UInt32(kSecCSCheckAllArchitectures) | UInt32(kSecCSCheckNestedCode))
      return SecStaticCodeCheckValidity(code, flags, nil) == errSecSuccess
    }
  }
#endif
