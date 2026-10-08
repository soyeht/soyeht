import Foundation
import os

/// Failures the client recovers from on its own but the logs should still show.
let clientLogger = Logger(subsystem: "com.soyeht.next", category: "client")

public enum ClientDiagnostics {
  private static let reported = OSAllocatedUnfairLock(initialState: Set<String>())

  /// Logs `event` once per distinct `key`. Views recompute layouts on every render, so a
  /// failure that repeats there must not flood the log. Returns whether it logged now.
  @discardableResult
  public static func reportOnce(_ event: String, key: String, error: any Error) -> Bool {
    guard reported.withLock({ $0.insert(event + "\u{0}" + key).inserted }) else { return false }
    clientLogger.error(
      "\(event, privacy: .public) error=\(String(describing: error), privacy: .public)")
    return true
  }

  /// Decodes `data`, logging why it could not be decoded instead of dropping the reason.
  static func decoded<T: Decodable>(_ type: T.Type, from data: Data, event: String) -> T? {
    do {
      return try JSONDecoder().decode(type, from: data)
    } catch {
      clientLogger.error(
        "\(event, privacy: .public) bytes=\(data.count) error=\(String(describing: error), privacy: .public)"
      )
      return nil
    }
  }

  /// `try?` for a value the screen can do without, with the failure logged once per key.
  public static func attempt<T>(_ event: String, key: String, _ body: () throws -> T) -> T? {
    do { return try body() } catch {
      reportOnce(event, key: key, error: error)
      return nil
    }
  }
}
