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
  public static let scanInstruction =
    "Open Soyeht Dev on your \(devices) and scan this code.\nBoth devices must be on the same tailnet."

  /// The onboarding line of a device that has no Mac yet: where to find the QR code and what to
  /// do with it. `device` is the name of the device the person holds (`DictationPlatform.title`).
  public static func onboarding(on device: String) -> String {
    "On your Mac, open Soyeht Dev and choose “\(pairButton)”. Then scan the code with this \(device) and confirm access on the Mac."
  }
}
