import Foundation

/// Bridges a system API that answers on a queue of its own into `async`.
///
/// Speech, microphone and similar prompts call their handler on a background queue. A closure
/// written inside a main-actor type inherits that isolation, and Swift 6 then traps with
/// "Block was expected to execute on queue com.apple.main-thread" as soon as the system calls
/// it. Both closures here are `@Sendable`, so they never take an actor from their context.
enum SystemCallback {
  static func value<Value: Sendable>(
    _ register: @Sendable (@escaping @Sendable (Value) -> Void) -> Void
  ) async -> Value {
    await withCheckedContinuation { continuation in
      register { continuation.resume(returning: $0) }
    }
  }
}
