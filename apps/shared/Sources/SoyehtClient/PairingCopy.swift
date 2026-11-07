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
  /// (ADR 0152): the first twelve hexadecimal digits of the key's fingerprint, in capitals, in
  /// three groups of four. `nil` for a fingerprint that has no twelve hexadecimal digits.
  /// `contracts/vectors/pairing-comparison.json` holds the cases; the Rust client passes them too.
  public static func comparisonCode(_ fingerprint: String) -> String? {
    let head = fingerprint.prefix(comparisonDigits)
    guard head.count == comparisonDigits, head.allSatisfy({ $0.isASCII && $0.isHexDigit })
    else { return nil }
    let capitals = Array(head.uppercased())
    return stride(from: 0, to: comparisonDigits, by: comparisonGroup)
      .map { String(capitals[$0..<($0 + comparisonGroup)]) }
      .joined(separator: " ")
  }
  private static let comparisonDigits = 12
  private static let comparisonGroup = 4

  /// What the new device says while the person compares the code with the Mac's.
  public static let compareOnDevice =
    "Check that the Mac shows this code, then allow it there. If the Mac shows another code or none, tap Cancel."
  /// What the Mac says about the request it shows: the code is the one the person's own device
  /// shows.
  public static let compareOnMac =
    "Allow only if your \(devices) shows this same code. If it shows another code or none, deny."
}
