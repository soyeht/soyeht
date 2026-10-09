import Foundation
import Observation

/// The person's dictation vocabulary: project names, commands, people. An engine that
/// supports it favors these words when it chooses between candidates that sound alike.
public enum DictationVocabulary {
  /// Biasing dilutes as the list grows, and every phrase costs work for each emitted token.
  /// A couple of hundred terms of real jargon is the useful range.
  public static let phraseLimit = 200

  /// Trims, drops blanks and exact duplicates, keeps the order and the casing, and caps the
  /// list.
  public static func sanitize(_ phrases: [String]) -> [String] {
    var seen = Set<String>()
    var result: [String] = []
    for phrase in phrases {
      let trimmed = phrase.trimmingCharacters(in: .whitespacesAndNewlines)
      guard !trimmed.isEmpty, seen.insert(trimmed).inserted else { continue }
      result.append(trimmed)
      if result.count == phraseLimit { break }
    }
    return result
  }
}

/// The vocabulary on this device. Every engine that uses a vocabulary reads this one list.
@MainActor @Observable public final class DictationVocabularyStore {
  public enum AddResult: Equatable, Sendable {
    case added
    case duplicate
    case empty
    case full
  }

  public private(set) var phrases: [String]
  @ObservationIgnored private let defaults: UserDefaults
  @ObservationIgnored private let key: String

  public init(defaults: UserDefaults = .standard, key: String = "soyeht.dictation.vocabulary") {
    self.defaults = defaults
    self.key = key
    phrases = DictationVocabulary.sanitize(defaults.stringArray(forKey: key) ?? [])
  }

  @discardableResult
  public func add(_ raw: String) -> AddResult {
    let phrase = raw.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !phrase.isEmpty else { return .empty }
    guard !phrases.contains(phrase) else { return .duplicate }
    guard phrases.count < DictationVocabulary.phraseLimit else { return .full }
    phrases.append(phrase)
    save()
    return .added
  }

  public func remove(_ phrase: String) {
    phrases.removeAll { $0 == phrase }
    save()
  }

  private func save() {
    defaults.set(phrases, forKey: key)
  }
}
