import Foundation

/// The parse of every reply a transcript shows, kept by text. A transcript re-evaluates its rows
/// each time any message changes, and a reply that has not changed must not be parsed again.
enum MarkdownReplyCache {
  private final class Parsed {
    let blocks: [MarkdownBlock]
    init(_ blocks: [MarkdownBlock]) { self.blocks = blocks }
  }

  private nonisolated(unsafe) static let cache: NSCache<NSString, Parsed> = {
    let cache = NSCache<NSString, Parsed>()
    cache.countLimit = 512
    return cache
  }()

  /// How many replies were parsed rather than found, for the tests that bound the work.
  private(set) nonisolated(unsafe) static var parseCount = 0

  static func blocks(for text: String, streaming: Bool) -> [MarkdownBlock] {
    let key = (streaming ? "s" : "f") + text as NSString
    if let known = cache.object(forKey: key) { return known.blocks }
    parseCount += 1
    let parsed = MarkdownReply.parse(text, streaming: streaming)
    cache.setObject(Parsed(parsed), forKey: key)
    return parsed
  }
}
