import Foundation
import SoyehtProtocol
import Testing
import UserNotifications

@testable import SoyehtClient

/// The payload the engine builds for a permission request (shared with its Rust test).
private func enginePayload() throws -> [AnyHashable: Any] {
  let vector = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("contracts/vectors/push-permission.json")
  let object = try JSONSerialization.jsonObject(with: Data(contentsOf: vector))
  return try #require(object as? [AnyHashable: Any])
}

private let engine = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d"
private let pane = "0b9f1e4c-5a6d-4c3b-8e2f-1a2b3c4d5e6f"
private let request = "6f1c7a2e-0d5b-4e8f-9a51-2c3d4e5f6a7b"

@Test func notificationActionsAnswerWithTheEngineOptions() throws {
  let payload = try enginePayload()
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: PushCategory.allowAction, engineId: engine)
      == .answer(paneId: pane, requestId: request, optionId: "allow"))
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: PushCategory.denyAction, engineId: engine)
      == .answer(paneId: pane, requestId: request, optionId: "reject"))
  #expect(PushNotificationRoute.requestId(payload) == request)
}

@Test func tappingTheNotificationOpensThePaneAndDismissingDoesNothing() throws {
  let payload = try enginePayload()
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: UNNotificationDefaultActionIdentifier, engineId: engine)
      == .open(workspaceId: "3c2b1a0f-9e8d-4c7b-a6f5-e4d3c2b1a0f9", paneId: pane))
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: UNNotificationDismissActionIdentifier, engineId: engine)
      == .ignore)
}

@Test func anotherMacsOrAnIncompleteNotificationIsIgnored() throws {
  let payload = try enginePayload()
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: PushCategory.allowAction, engineId: "another-engine")
      == .ignore)
  #expect(
    PushNotificationRoute(
      userInfo: payload, actionIdentifier: PushCategory.allowAction, engineId: nil) == .ignore)
  var question = try #require(payload["soyeht"] as? [String: Any])
  question.removeValue(forKey: "allowOptionId")
  #expect(
    PushNotificationRoute(
      userInfo: ["soyeht": question], actionIdentifier: PushCategory.allowAction, engineId: engine)
      == .ignore)
  #expect(
    PushNotificationRoute(
      userInfo: [:], actionIdentifier: PushCategory.allowAction, engineId: engine)
      == .ignore)
}

@Test func permissionCategoryNeedsAnUnlockedDeviceAndStaysInTheBackground() throws {
  let payload = try enginePayload()
  let aps = try #require(payload["aps"] as? [String: Any])
  let categories = PushCategory.categories()
  let permission = try #require(categories.first { $0.identifier == aps["category"] as? String })
  #expect(
    permission.actions.map(\.identifier) == [PushCategory.allowAction, PushCategory.denyAction])
  #expect(permission.actions.map(\.title) == ["Allow", "Deny"])
  for action in permission.actions {
    #expect(action.options.contains(.authenticationRequired))
    #expect(!action.options.contains(.foreground))
    #expect(!action.options.contains(.destructive))
  }
  let attention = try #require(categories.first { $0.identifier == PushCategory.attention })
  #expect(attention.actions.isEmpty)
}

@Test func deviceTokenIsLowercaseHex() {
  #expect(PushToken.hex(Data([0x00, 0xfc, 0x13, 0xad])) == "00fc13ad")
}

@Test func provisioningProfileSelectsTheApnsEnvironment() {
  func profile(_ environment: String) -> Data {
    Data("signed-prefix".utf8)
      + Data(
        """
        <?xml version="1.0" encoding="UTF-8"?>
        <plist version="1.0"><dict><key>Entitlements</key><dict>
        <key>aps-environment</key><string>\(environment)</string>
        </dict></dict></plist>
        """.utf8) + Data("signed-suffix".utf8)
  }
  #expect(
    PushToken.environment(provisioningProfile: profile("development"), simulator: false) == .sandbox
  )
  #expect(
    PushToken.environment(provisioningProfile: profile("production"), simulator: false)
      == .production)
  #expect(PushToken.environment(provisioningProfile: nil, simulator: false) == .production)
  #expect(PushToken.environment(provisioningProfile: nil, simulator: true) == .sandbox)
}
