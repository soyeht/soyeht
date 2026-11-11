/// What the Mac and the paired devices call each other while pairing. A paired device is an
/// iPhone or an iPad, so the Mac names both and a device names itself (`DictationPlatform` is the
/// one place that knows which device this is).
public enum PairingCopy {
  /// The Mac's name for the people it pairs with.
  public static let devices = "iPhone or iPad"
  /// The Mac's button that starts a pairing.
  public static let pairButton = "Pair \(devices)"
  /// The Mac's heading for the section and the sheet.
  public static let pairedDevicesTitle = "Paired devices"
  public static let noPairedDevice = "No paired device"
  public static let continueTitle = "Continue on \(devices)"
  public static let revokeQuestion = "Revoke this device's access?"
  /// The line under the QR code on the Mac.
  public static var scanInstruction: String {
    "Open \(AppName.display()) on your \(devices) and scan this code.\nBoth devices must be on the same tailnet."
  }

  /// The onboarding line of a device that has no Mac yet: where to find the QR code and what to
  /// do with it. `device` is the name of the device the person holds (`DictationPlatform.title`).
  public static func onboarding(on device: String) -> String {
    "On your Mac, open \(AppName.display()) and choose “\(pairButton)”. Then scan the code with this \(device) and confirm access on the Mac."
  }

  /// The short code a pairing shows on the new device and on the Mac, for the person to compare
  /// (ADR 0152): the first sixteen hexadecimal digits of the key's fingerprint, in capitals, in
  /// four groups of four. `nil` unless the whole fingerprint is plain ASCII hexadecimal digits and
  /// has at least sixteen: it reads bytes, as the Rust client does, so a combining mark or a
  /// multi-byte character cannot make the two disagree.
  /// `contracts/vectors/pairing-comparison.json` holds the cases; the Rust client passes them too.
  public static func comparisonCode(_ fingerprint: String) -> String? {
    let bytes = Array(fingerprint.utf8)
    guard bytes.count >= comparisonDigits, bytes.allSatisfy(isAsciiHexDigit) else { return nil }
    let capitals = Array(String(decoding: bytes[..<comparisonDigits], as: UTF8.self).uppercased())
    return stride(from: 0, to: comparisonDigits, by: comparisonGroup)
      .map { String(capitals[$0..<($0 + comparisonGroup)]) }
      .joined(separator: " ")
  }
  private static func isAsciiHexDigit(_ byte: UInt8) -> Bool {
    (0x30...0x39).contains(byte) || (0x41...0x46).contains(byte) || (0x61...0x66).contains(byte)
  }
  private static let comparisonDigits = 16
  private static let comparisonGroup = 4

  /// The device's line while it waits for the person to allow it on the Mac: the code is the thing
  /// to look at, not a fingerprint.
  public static let waitingForTheMac = "Compare the code and allow access on the Mac."
  /// What the new device says while the person compares the code with the Mac's.
  public static let compareOnDevice =
    "Check that the Mac shows this code, then allow it there. If the Mac shows another code or none, tap Cancel."
  /// The Mac's question for a request to pair, wherever it is shown.
  public static func allowQuestion(_ deviceName: String) -> String {
    "Allow access for \(deviceName)?"
  }
  /// What the Mac says about a request whose key has no code to compare.
  public static let noComparisonCode =
    "This request has no code to compare, so it cannot be checked. Deny it."
  /// What the Mac says about the request it shows: the code is the one the person's own device
  /// shows.
  public static let compareOnMac =
    "Allow only if your \(devices) shows this same code. If it shows another code or none, deny."
}
