import SoyehtProtocol

/// How long the client waits before it connects to the engine again. A refusal that repeats
/// is not retried every second forever: the wait doubles up to a ceiling, and a connection
/// that was established starts it over.
struct ReconnectBackoff: Equatable, Sendable {
  static let first: Duration = .milliseconds(ClientPolicy.reconnectDelayMs)
  static let ceiling: Duration = .milliseconds(ClientPolicy.reconnectCeilingMs)

  private var delay = Self.first

  /// The wait before the next attempt; the one after it is twice as long, up to the ceiling.
  mutating func next() -> Duration {
    defer { delay = min(delay * 2, Self.ceiling) }
    return delay
  }

  mutating func reset() { delay = Self.first }
}
