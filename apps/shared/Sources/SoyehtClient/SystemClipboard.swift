import Foundation

#if os(macOS)
  import AppKit
#else
  import UIKit
#endif

/// The system pasteboard behind one call, the same on the Mac and on iOS.
enum SystemClipboard {
  /// Replaces what the pasteboard holds with `text`, as plain text.
  @MainActor static func copy(_ text: String) {
    #if os(macOS)
      NSPasteboard.general.clearContents()
      NSPasteboard.general.setString(text, forType: .string)
    #else
      UIPasteboard.general.string = text
    #endif
  }
}
