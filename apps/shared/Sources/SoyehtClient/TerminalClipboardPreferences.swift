import Foundation
import Observation

/// Whether a program running in the terminal may put text on this device's clipboard (an
/// OSC 52 write). Off by default: the clipboard may be shared with the person's other devices
/// (Universal Clipboard), so a program, a file shown with `cat` or an SSH session must not
/// replace it unasked.
@MainActor @Observable
public final class TerminalClipboardPreferences {
  public static let allowKey = "soyeht.terminal.programsMayCopy"
  public static let shared = TerminalClipboardPreferences()

  public private(set) var allowsProgramCopy: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public init(preferences: any ClientPreferenceStore = UserDefaultsClientPreferenceStore()) {
    self.preferences = preferences
    allowsProgramCopy = preferences.bool(forKey: Self.allowKey)
  }

  public func setAllowsProgramCopy(_ allowed: Bool) {
    allowsProgramCopy = allowed
    preferences.set(allowed, forKey: Self.allowKey)
  }
}

/// The short "Copied by the terminal" notice shown when a program wrote to the clipboard.
@MainActor @Observable
public final class TerminalCopyNotice {
  public static let shared = TerminalCopyNotice()
  public static let text = "Copied by the terminal"

  public private(set) var visible = false
  @ObservationIgnored private let duration: Duration
  /// The pending hide, so a test can wait for it instead of for a fixed time.
  @ObservationIgnored private(set) var hiding: Task<Void, Never>?

  public init(duration: Duration = .seconds(2)) { self.duration = duration }

  public func show() {
    visible = true
    hiding?.cancel()
    hiding = Task { [weak self, duration] in
      do { try await Task.sleep(for: duration) } catch { return }  // a newer copy took over
      self?.visible = false
    }
  }
}
