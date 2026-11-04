import Foundation

/// The name the person sees under the app's icon, which differs by build: "Soyeht Dev" for the
/// Dev profile and the store build's own name. Text that tells the person to open the app reads
/// it here, so no string names a build.
enum AppName {
  static let fallback = "Soyeht"

  static func display(info: [String: Any]? = Bundle.main.infoDictionary) -> String {
    guard let name = info?["CFBundleDisplayName"] as? String, !name.isEmpty else { return fallback }
    return name
  }
}
