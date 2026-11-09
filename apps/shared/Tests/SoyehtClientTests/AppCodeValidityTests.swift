#if os(macOS)
  import Foundation
  import Security
  import Testing

  @testable import SoyehtClient

  /// A signed app of the system, only read and copied: nothing is opened or launched.
  private let signedApp = URL(fileURLWithPath: "/System/Applications/Calculator.app")
  private let present = FileManager.default.fileExists(atPath: signedApp.path)

  /// The designated requirement of the signed app, standing in for the running app's.
  private func requirement(of app: URL) throws -> SecRequirement {
    var code: SecStaticCode?
    var requirement: SecRequirement?
    #expect(SecStaticCodeCreateWithPath(app as CFURL, SecCSFlags(), &code) == errSecSuccess)
    let made = try #require(code)
    #expect(SecCodeCopyDesignatedRequirement(made, SecCSFlags(), &requirement) == errSecSuccess)
    return try #require(requirement)
  }

  /// The name the mover gives its staging copy: hidden, with no `.app` extension.
  private func stagingName() -> String { ".\(testAppName).moving.\(UUID().uuidString)" }

  @Test(.enabled(if: present))
  func theSystemCheckAcceptsASignedAppAndRefusesACopyThatWasChanged() throws {
    let wanted = try requirement(of: signedApp)
    #expect(SystemCodeValidity.isValid(signedApp, matching: wanted))

    let box = try MoveSandbox()
    let copy = try box.folder("copy").appending(path: "Calculator.app")
    try FileManager.default.copyItem(at: signedApp, to: copy)
    #expect(SystemCodeValidity.isValid(copy, matching: wanted), "a whole copy keeps its seal")
    let plist = copy.appending(path: "Contents/Info.plist")
    let handle = try FileHandle(forWritingTo: plist)
    try handle.seekToEnd()
    try handle.write(contentsOf: Data("<!-- changed -->".utf8))
    try handle.close()
    #expect(
      !SystemCodeValidity.isValid(copy, matching: wanted),
      "a copy that differs from what was sealed is refused")
    #expect(!SystemCodeValidity.isValid(copy, matching: nil), "the seal alone is broken too")
  }

  @Test(.enabled(if: present))
  func theCheckWorksOnTheHiddenNameWithNoExtensionTheMoverStagesUnder() throws {
    let wanted = try requirement(of: signedApp)
    let box = try MoveSandbox()
    let staged = try box.folder("Applications").appending(path: stagingName())
    try FileManager.default.copyItem(at: signedApp, to: staged)
    #expect(staged.pathExtension != "app", "the staging name has no bundle extension")
    #expect(SystemCodeValidity.isValid(staged, matching: wanted))
    #expect(SystemCodeValidity.isValid(staged, matching: nil))
  }

  @Test(.enabled(if: present))
  func aSealedCopySignedByAnotherSignerIsRefused() throws {
    let wanted = try requirement(of: signedApp)
    let box = try MoveSandbox()
    let copy = try box.folder("copy").appending(path: stagingName())
    try FileManager.default.copyItem(at: signedApp, to: copy)
    try adHocSign(copy)
    #expect(SystemCodeValidity.isValid(copy, matching: nil), "the new seal is whole")
    #expect(!SystemCodeValidity.isValid(copy, matching: wanted), "ad hoc is not the wanted signer")
  }

  @Test(.enabled(if: present))
  func aRequirementOfAnotherIdentityRefusesAWholeBundle() throws {
    var other: SecRequirement?
    #expect(
      SecRequirementCreateWithString(
        "identifier \"com.example.somebody-else\"" as CFString, SecCSFlags(), &other)
        == errSecSuccess)
    let box = try MoveSandbox()
    let copy = try box.folder("copy").appending(path: "Calculator.app")
    try FileManager.default.copyItem(at: signedApp, to: copy)
    #expect(SystemCodeValidity.isValid(copy, matching: nil))
    #expect(!SystemCodeValidity.isValid(copy, matching: other))
  }

  @Test(.enabled(if: present))
  func theProductionCheckAsksForTheRunningCodesRequirement() throws {
    // The tests run inside a tool that is not the app: its requirement is not the copy's, so the
    // copy that is whole is still refused. A nil requirement would have passed it.
    let box = try MoveSandbox()
    let copy = try box.folder("copy").appending(path: stagingName())
    try FileManager.default.copyItem(at: signedApp, to: copy)
    #expect(SystemCodeValidity.isValid(copy, matching: nil))
    #expect(!SystemCodeValidity().isValid(copy))
  }

  @Test func theSystemCheckRefusesAnAppThatIsNotSigned() throws {
    let box = try MoveSandbox()
    let app = try box.app(in: try box.folder("unsigned"))
    let wanted = try? requirement(of: signedApp)
    #expect(!SystemCodeValidity.isValid(app, matching: wanted))
    #expect(!SystemCodeValidity.isValid(app, matching: nil))
    #expect(!SystemCodeValidity().isValid(app))
    #expect(!SystemCodeValidity().isValid(box.root.appending(path: "missing.app")))
  }

  /// Signs the bundle with the ad hoc identity (`-`): no certificate, no keychain.
  private func adHocSign(_ bundle: URL) throws {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
    process.arguments = ["--force", "--deep", "--sign", "-", bundle.path]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    try process.run()
    process.waitUntilExit()
    #expect(process.terminationStatus == 0, "ad hoc signing of the temporary copy")
  }
#endif
