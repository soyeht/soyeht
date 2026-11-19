import Foundation
import SoyehtProtocol

/// Ranges of code points from a list of `x-clientPolicy.voice` ("1F000-1FAFF|20E3": a range or one
/// code point, separated by `|`).
struct ScalarRanges: Sendable {
  private let ranges: [ClosedRange<UInt32>]

  init(policy: String) {
    ranges = policy.split(separator: "|").compactMap { entry in
      let ends = entry.split(separator: "-").compactMap { UInt32($0, radix: 16) }
      guard let first = ends.first, let last = ends.last else { return nil }
      return first...last
    }
  }

  func contains(_ scalar: Unicode.Scalar) -> Bool {
    ranges.contains { $0.contains(scalar.value) }
  }
}

/// How the turn machine compares words: Unicode NFC, lowercase, a word being a run of letters,
/// marks and digits, and anything else a break. Each character of a script that writes no blanks
/// (Han, kana, Thai, Lao, Khmer, Myanmar) is a word of its own.
enum VoiceText {
  static func tokens(_ text: String) -> [String] {
    var words: [String] = []
    var current = ""
    for scalar in text.precomposedStringWithCanonicalMapping.lowercased().unicodeScalars {
      if isBlankless(scalar) {
        if !current.isEmpty { words.append(current) }
        current = ""
        words.append(String(scalar))
      } else if CharacterSet.alphanumerics.contains(scalar) {
        current.unicodeScalars.append(scalar)
      } else {
        if !current.isEmpty { words.append(current) }
        current = ""
      }
    }
    if !current.isEmpty { words.append(current) }
    return words
  }

  /// A script whose words are not set apart by blanks.
  static func isBlankless(_ scalar: Unicode.Scalar) -> Bool { blankless.contains(scalar) }

  private static let blankless = ScalarRanges(policy: ClientPolicy.voiceScriptsWithoutBlanks)
}
