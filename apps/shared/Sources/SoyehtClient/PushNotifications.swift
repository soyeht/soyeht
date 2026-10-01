import Foundation
import SoyehtProtocol
@preconcurrency import UserNotifications

/// Notification categories and actions for the alerts the engine sends when an agent
/// asks the person something. The identifiers match the engine's payload
/// (`engine/soyeht-engine/src/push/payload.rs`).
public enum PushCategory {
  /// Allow and Deny actions; answering does not open the app.
  public static let permission = "soyeht.permission"
  /// No actions: a tap opens the pane.
  public static let attention = "soyeht.attention"
  public static let allowAction = "soyeht.permission.allow"
  public static let denyAction = "soyeht.permission.deny"

  /// Both actions run a command on the Mac, so the device must be unlocked first. Deny is
  /// not destructive: it refuses the tool call and changes nothing.
  public static func categories() -> Set<UNNotificationCategory> {
    let allow = UNNotificationAction(
      identifier: allowAction, title: "Allow", options: [.authenticationRequired])
    let deny = UNNotificationAction(
      identifier: denyAction, title: "Deny", options: [.authenticationRequired])
    return [
      UNNotificationCategory(
        identifier: permission, actions: [allow, deny], intentIdentifiers: [],
        hiddenPreviewsBodyPlaceholder: "Approval needed", options: []),
      UNNotificationCategory(
        identifier: attention, actions: [], intentIdentifiers: [],
        hiddenPreviewsBodyPlaceholder: "An agent needs you", options: []),
    ]
  }
}

/// What a notification response asks the app to do.
public enum PushNotificationRoute: Equatable, Sendable {
  /// Answer the permission request through `pane.permission.answer`.
  case answer(paneId: String, requestId: String, optionId: String)
  /// Show the pane so the person continues there.
  case open(workspaceId: String, paneId: String)
  case ignore

  /// - Parameters:
  ///   - userInfo: the notification payload; the engine's keys live under `soyeht`.
  ///   - actionIdentifier: the chosen action, or the system's default or dismiss action.
  ///   - engineId: the paired Mac; a notification from another engine is ignored.
  public init(userInfo: [AnyHashable: Any], actionIdentifier: String, engineId: String?) {
    guard let info = userInfo["soyeht"] as? [String: Any],
      let engine = info["engineId"] as? String, engine == engineId,
      let workspaceId = info["workspaceId"] as? String,
      let paneId = info["paneId"] as? String,
      let requestId = info["requestId"] as? String
    else {
      self = .ignore
      return
    }
    let option: String? =
      switch actionIdentifier {
      case PushCategory.allowAction: info["allowOptionId"] as? String
      case PushCategory.denyAction: info["denyOptionId"] as? String
      default: nil
      }
    if let option {
      self = .answer(paneId: paneId, requestId: requestId, optionId: option)
    } else if actionIdentifier == UNNotificationDefaultActionIdentifier {
      self = .open(workspaceId: workspaceId, paneId: paneId)
    } else {
      self = .ignore
    }
  }

  /// The request a delivered notification asks about, if it came from the engine.
  public static func requestId(_ userInfo: [AnyHashable: Any]) -> String? {
    (userInfo["soyeht"] as? [String: Any])?["requestId"] as? String
  }
}

public enum PushToken {
  /// The hexadecimal form APNs expects in `/3/device/<token>`.
  public static func hex(_ token: Data) -> String {
    token.map { String(format: "%02x", $0) }.joined()
  }

  /// The APNs environment this build's tokens belong to. A development provisioning
  /// profile signs for the sandbox; the App Store build has no profile and uses production.
  public static func environment(provisioningProfile: Data?, simulator: Bool) -> PushEnvironment {
    guard let profile = provisioningProfile else { return simulator ? .sandbox : .production }
    // The profile is a signed envelope around an XML property list.
    guard let start = profile.range(of: Data("<?xml".utf8)),
      let end = profile.range(of: Data("</plist>".utf8), in: start.lowerBound..<profile.endIndex),
      let plist = try? PropertyListSerialization.propertyList(
        from: profile.subdata(in: start.lowerBound..<end.upperBound), format: nil)
        as? [String: Any],
      let entitlements = plist["Entitlements"] as? [String: Any]
    else { return .sandbox }
    return entitlements["aps-environment"] as? String == "production" ? .production : .sandbox
  }
}
