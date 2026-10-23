import Foundation
import SoyehtProtocol

/// Which documents of a section a search box keeps, so every client lists the same ones.
public enum WritingSectionSearch {
  /// The entries whose title or path contains the trimmed `query`, ignoring case; all of them
  /// for a blank query.
  public static func filter(
    _ entries: [WritingSectionEntry], query: String
  ) -> [WritingSectionEntry] {
    let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !needle.isEmpty else { return entries }
    return entries.filter {
      $0.title.localizedCaseInsensitiveContains(needle)
        || $0.path.localizedCaseInsensitiveContains(needle)
    }
  }
}
