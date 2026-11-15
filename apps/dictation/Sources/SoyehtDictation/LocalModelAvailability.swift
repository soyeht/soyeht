import Foundation
import SoyehtClient

/// Whether a device can run a CoreML speech model on this device: the one rule every local
/// engine and voice shares. The CoreML bundles need an arm64 chip. On iPhone and iPad the weights
/// and the compiled models take more than a gigabyte of memory, so devices below 6 GB of RAM keep
/// the system's engine, until a model is measured on a phone.
enum LocalModelAvailability {
  @Sendable static func appleSiliconWithEnoughMemory() -> DictationAvailability {
    #if arch(arm64)
      #if os(iOS)
        guard ProcessInfo.processInfo.physicalMemory >= minimumMemory else {
          return .unavailable("Needs a device with at least 6 GB of memory")
        }
      #endif
      return .available
    #else
      return .unavailable("Needs Apple silicon")
    #endif
  }

  #if os(iOS)
    private static let minimumMemory: UInt64 = 6 << 30
  #endif
}
