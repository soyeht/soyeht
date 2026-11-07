#if os(macOS)
  import Foundation
  import Security

  /// Whether an app bundle's code signature is intact and is this app's own. A seam, so a test
  /// does not need a signed app.
  public protocol AppCodeValidating: Sendable {
    func isValid(_ app: URL) -> Bool
  }

  /// The system's own check of a bundle's signature (`SecStaticCodeCheckValidity`): the seal of
  /// the main executable, of every resource and of all nested code is whole, for every
  /// architecture, and the code satisfies the designated requirement of the running app. The
  /// requirement is what makes the signer count: a bundle that is sealed but signed by anyone
  /// else (or ad hoc) is refused, so the mover never clears the quarantine mark of, or relaunches,
  /// code that is not signed like this app. The requirement is read from the running code, not
  /// from the path it was opened by, which can be a link to something else.
  public struct SystemCodeValidity: AppCodeValidating {
    public init() {}

    public func isValid(_ app: URL) -> Bool {
      // No requirement for the running code (an unsigned build) is a refusal, never a pass.
      guard let requirement = Self.runningRequirement() else { return false }
      return Self.isValid(app, matching: requirement)
    }

    /// The designated requirement of the running code, or nil when it has none.
    static func runningRequirement() -> SecRequirement? {
      var current: SecCode?
      var code: SecStaticCode?
      var requirement: SecRequirement?
      guard SecCodeCopySelf(SecCSFlags(), &current) == errSecSuccess, let current,
        SecCodeCopyStaticCode(current, SecCSFlags(), &code) == errSecSuccess, let code,
        SecCodeCopyDesignatedRequirement(code, SecCSFlags(), &requirement) == errSecSuccess
      else { return nil }
      return requirement
    }

    /// The seal of `app` is whole and, when `requirement` is given, `app` satisfies it. A nil
    /// requirement checks the seal only: the production check never passes nil.
    static func isValid(_ app: URL, matching requirement: SecRequirement?) -> Bool {
      var code: SecStaticCode?
      guard SecStaticCodeCreateWithPath(app as CFURL, SecCSFlags(), &code) == errSecSuccess,
        let code
      else { return false }
      let flags = SecCSFlags(
        rawValue: UInt32(kSecCSCheckAllArchitectures) | UInt32(kSecCSCheckNestedCode))
      return SecStaticCodeCheckValidity(code, flags, requirement) == errSecSuccess
    }
  }
#endif
