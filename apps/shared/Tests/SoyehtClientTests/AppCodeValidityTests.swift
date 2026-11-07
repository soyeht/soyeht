#if os(macOS)
  import Foundation
  import Testing

  @testable import SoyehtClient

  /// A signed app of the system, only read and copied: nothing is opened or launched.
  private let signedApp = URL(fileURLWithPath: "/System/Applications/Calculator.app")

  @Test(.enabled(if: FileManager.default.fileExists(atPath: signedApp.path)))
  func theSystemCheckAcceptsASignedAppAndRefusesACopyThatWasChanged() throws {
    let check = SystemCodeValidity()
    #expect(check.isValid(signedApp))

    let box = try MoveSandbox()
    let copy = try box.folder("copy").appending(path: "Calculator.app")
    try FileManager.default.copyItem(at: signedApp, to: copy)
    #expect(check.isValid(copy), "a whole copy keeps its seal")
    let plist = copy.appending(path: "Contents/Info.plist")
    let handle = try FileHandle(forWritingTo: plist)
    try handle.seekToEnd()
    try handle.write(contentsOf: Data("<!-- changed -->".utf8))
    try handle.close()
    #expect(!check.isValid(copy), "a copy that differs from what was sealed is refused")
  }

  @Test func theSystemCheckRefusesAnAppThatIsNotSigned() throws {
    let box = try MoveSandbox()
    let app = try box.app(in: try box.folder("unsigned"))
    #expect(!SystemCodeValidity().isValid(app))
    #expect(!SystemCodeValidity().isValid(box.root.appending(path: "missing.app")))
  }
#endif
