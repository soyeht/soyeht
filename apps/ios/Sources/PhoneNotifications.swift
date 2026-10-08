import SoyehtClient
import UIKit
import UserNotifications
import os

private let pushLogger = Logger(subsystem: "com.soyeht.next", category: "push")

/// Owns the app's model so a notification action that launches the app in the background
/// finds it, and routes notification responses to it.
@MainActor
final class PhoneAppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
  let model = PhoneModel()

  func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
  ) -> Bool {
    // Categories are declared at launch; this asks for nothing.
    let center = UNUserNotificationCenter.current()
    center.delegate = self
    center.setNotificationCategories(PushCategory.categories())
    Task {
      // Refresh the token on every launch once the person has allowed notifications.
      let status = await center.notificationSettings().authorizationStatus
      if PhonePush.allowed(status) { application.registerForRemoteNotifications() }
    }
    return true
  }

  func application(
    _ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data
  ) {
    model.push.received(token: PushToken.hex(token), client: model.client)
  }

  func application(
    _ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error
  ) {
    pushLogger.error("remote notification registration failed: \(error.localizedDescription)")
  }

  nonisolated func userNotificationCenter(
    _ center: UNUserNotificationCenter, willPresent notification: UNNotification
  ) async -> UNNotificationPresentationOptions {
    [.banner, .list, .sound]
  }

  nonisolated func userNotificationCenter(
    _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse
  ) async {
    let content = response.notification.request.content
    // The engine's keys are all strings; copying them keeps the value sendable.
    let info = content.userInfo["soyeht"] as? [String: String] ?? [:]
    await respond(info: info, action: response.actionIdentifier, title: content.title)
  }

  /// The system keeps a background launch alive until this returns, so an answer is
  /// awaited here rather than in a detached task.
  private func respond(info: [String: String], action: String, title: String) async {
    model.resume()
    let route = PushNotificationRoute(
      userInfo: ["soyeht": info], actionIdentifier: action, engineId: model.client?.engineId)
    switch route {
    case .answer(let paneId, let requestId, let optionId):
      await model.answerFromNotification(
        paneId: paneId, requestId: requestId, optionId: optionId,
        retry: PhoneNotice(title: title, info: info))
    case .open(_, let paneId):
      model.requestedPaneId = paneId
    case .ignore:
      break
    }
  }
}

/// Notification authorization and the APNs token registered with the paired Mac.
@MainActor
final class PhonePush {
  private var token: String?
  private var registeredConnection: UInt64?

  static func allowed(_ status: UNAuthorizationStatus) -> Bool {
    [.authorized, .provisional, .ephemeral].contains(status)
  }

  /// Called each time the paired Mac connects. The first time, after pairing, it asks for
  /// permission to notify; then it (re)registers the device token with the Mac.
  func connected(_ client: EngineClient) async {
    let center = UNUserNotificationCenter.current()
    var status = await center.notificationSettings().authorizationStatus
    if status == .notDetermined {
      do {
        _ = try await center.requestAuthorization(options: [.alert, .sound, .badge])
      } catch {
        // The settings read below decides; the reason must still reach the log.
        pushLogger.error("notification_authorization_failed error=\(error.localizedDescription)")
      }
      status = await center.notificationSettings().authorizationStatus
    }
    guard Self.allowed(status) else { return }
    UIApplication.shared.registerForRemoteNotifications()
    await register(client)
  }

  func received(token: String, client: EngineClient?) {
    guard token != self.token else { return }
    self.token = token
    registeredConnection = nil
    if let client { Task { await register(client) } }
  }

  private func register(_ client: EngineClient) async {
    guard let token, client.connected, registeredConnection != client.connectionRevision else {
      return
    }
    #if targetEnvironment(simulator)
      let simulator = true
    #else
      let simulator = false
    #endif
    let environment = PushToken.environment(
      provisioningProfile: Bundle.main.url(
        forResource: "embedded", withExtension: "mobileprovision"
      )
      .flatMap { try? Data(contentsOf: $0) },
      simulator: simulator)
    do {
      try await client.registerPush(token: token, environment: environment)
      registeredConnection = client.connectionRevision
    } catch {
      pushLogger.error("push registration failed: \(error.localizedDescription)")
    }
  }

  /// Removes delivered alerts whose question was answered elsewhere, expired or cancelled.
  static func removeSettled(open: Set<String>) async {
    let center = UNUserNotificationCenter.current()
    let settled = await center.deliveredNotifications().filter { notification in
      PushNotificationRoute.requestId(notification.request.content.userInfo)
        .map { !open.contains($0) } ?? false
    }
    center.removeDeliveredNotifications(withIdentifiers: settled.map(\.request.identifier))
  }
}

/// A local copy of an alert whose answer did not reach the Mac, with the same actions, so
/// the person can try again or open the pane.
struct PhoneNotice: Sendable {
  let title: String
  let info: [String: String]

  func postUndelivered() async {
    let content = UNMutableNotificationContent()
    content.title = title
    content.body = "The answer did not reach your Mac. Try again or open the pane."
    content.categoryIdentifier = PushCategory.permission
    content.threadIdentifier = info["paneId"] ?? ""
    content.userInfo = ["soyeht": info]
    let request = UNNotificationRequest(
      identifier: info["requestId"] ?? UUID().uuidString, content: content, trigger: nil)
    do { try await UNUserNotificationCenter.current().add(request) } catch {
      pushLogger.error("undelivered answer notice failed: \(error.localizedDescription)")
    }
  }
}
