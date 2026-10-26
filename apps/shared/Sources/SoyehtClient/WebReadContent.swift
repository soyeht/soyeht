import Foundation
import SoyehtProtocol

/// A run of visible text the page script found on one screen.
public struct WebReadBlock: Decodable, Sendable, Equatable {
  public var y: Int
  public var text: String
  public var pinned: Bool

  public init(y: Int, text: String, pinned: Bool) {
    self.y = y
    self.text = text
    self.pinned = pinned
  }
}

/// The text and links of every screen a read visited, joined once and within the limits.
///
/// Screens overlap and a fixed header repeats on each of them, so a block already seen at the
/// same page position (or a pinned block with the same words) is kept once.
public struct WebReadAccumulator: Sendable {
  public private(set) var lines: [String] = []
  public private(set) var links: [WebReadLink] = []
  public private(set) var truncated = false
  private var textBytes = 0
  private var seenBlocks: Set<String> = []
  private var seenLinks: Set<String> = []

  public init() {}

  public var text: String { lines.joined(separator: "\n") }

  /// A limit outside the accumulator (the page script's node cap) cut what it was given.
  public mutating func markTruncated() { truncated = true }

  public mutating func add(blocks: [WebReadBlock], links found: [WebReadLink]) {
    for block in blocks where !block.text.isEmpty {
      let key = block.pinned ? "pinned|\(block.text)" : "\(block.y)|\(block.text)"
      guard seenBlocks.insert(key).inserted else { continue }
      let cost = block.text.utf8.count + (lines.isEmpty ? 0 : 1)
      guard textBytes + cost <= ProtocolLimits.webReadTextBytes else {
        truncated = true
        continue
      }
      lines.append(block.text)
      textBytes += cost
    }
    for link in found {
      guard let bounded = boundedLink(link) else { continue }
      guard seenLinks.insert("\(bounded.href)|\(bounded.text)").inserted else { continue }
      guard links.count < ProtocolLimits.webReadLinks else {
        truncated = true
        continue
      }
      links.append(bounded)
    }
  }

  private mutating func boundedLink(_ link: WebReadLink) -> WebReadLink? {
    guard !link.href.isEmpty else { return nil }
    guard link.href.utf8.count <= ProtocolLimits.webFieldBytes else {
      truncated = true
      return nil
    }
    var text = link.text
    while text.utf8.count > ProtocolLimits.webTokenBytes { text.removeLast() }
    return WebReadLink(text: text, href: link.href)
  }
}

/// One screen's picture and where on the page it was taken, in view points.
///
/// `topInset` and `bottomInset` are the heights of what stays fixed on the screen (a sticky header,
/// a fixed bar): they repeat in every picture, so only the first picture keeps its top band and
/// only the last keeps its bottom band.
public struct WebReadTile: Sendable, Equatable {
  public var offset: Double
  public var height: Double
  public var topInset: Double
  public var bottomInset: Double

  public init(offset: Double, height: Double, topInset: Double = 0, bottomInset: Double = 0) {
    self.offset = offset
    self.height = height
    self.topInset = topInset
    self.bottomInset = bottomInset
  }
}

/// The rows of one tile that the stitched image keeps.
public struct WebReadStrip: Sendable, Equatable {
  public var tile: Int
  public var sourceY: Double
  public var destinationY: Double
  public var height: Double
}

public enum WebReadStitch {
  /// Each tile contributes only the rows below what the tiles before it already cover, and not
  /// the bands that stay fixed on every screen (except the first screen's top and the last
  /// screen's bottom).
  public static func strips(_ tiles: [WebReadTile]) -> (strips: [WebReadStrip], height: Double) {
    guard let first = tiles.first else { return ([], 0) }
    var covered = first.offset
    var strips: [WebReadStrip] = []
    for (index, tile) in tiles.enumerated() {
      let kept = index == 0 ? 0 : tile.topInset
      let dropped = index == tiles.count - 1 ? 0 : tile.bottomInset
      let top = tile.offset + kept
      let bottom = tile.offset + tile.height - dropped
      let start = max(covered, top)
      guard bottom > start else { continue }
      strips.append(
        WebReadStrip(
          tile: index, sourceY: start - tile.offset, destinationY: start - first.offset,
          height: bottom - start))
      covered = bottom
    }
    return (strips, covered - first.offset)
  }

  /// Pixels per view point of the stitched image: the screen's own density, lowered so the whole
  /// page stays inside the pixel budget.
  public static func outputScale(
    width: Double, height: Double, nativeScale: Double, pixelBudget: Double
  ) -> Double {
    guard width > 0, height > 0 else { return nativeScale }
    return min(nativeScale, (pixelBudget / (width * height)).squareRoot())
  }
}

/// The value a `web.read` command returns. The engine adds `paneId` itself, so it is not here.
///
/// The engine refuses a result over `webReadBytes` once JSON has escaped it, so the value is
/// measured as it will travel and the last links, then the last lines of text, are dropped until
/// it fits: a long page never fails the read for size.
public enum WebReadAssembly {
  /// What the engine's envelope adds around the value (the pane identity and the keys).
  static let envelopeBytes = 1_024

  public static func value(
    text: String, links: [WebReadLink], screens: Int, endOfPage: Bool, truncated: Bool,
    generation: UInt64, title: String, url: String, image: AttachmentRef
  ) throws -> JsonValue {
    var text = text
    var links = links
    var truncated = truncated
    while true {
      let body = WebReadResult(
        paneId: "", navigationGeneration: String(generation),
        title: String(title.prefix(ProtocolLimits.webTitleBytes)),
        url: String(url.prefix(ProtocolLimits.webFieldBytes)), screens: UInt32(screens),
        text: text, links: links, endOfPage: endOfPage, truncated: truncated, image: image)
      let encoded = try JSONEncoder().encode(body)
      if encoded.count + envelopeBytes <= ProtocolLimits.webReadBytes
        || (text.isEmpty && links.isEmpty)
      {
        guard case .object(var fields) = try JSONDecoder().decode(JsonValue.self, from: encoded)
        else { return .null }
        fields.removeValue(forKey: "paneId")
        return .object(fields)
      }
      truncated = true
      if !links.isEmpty {
        links.removeLast(max(1, links.count / 8))
      } else {
        text = shortened(text)
      }
    }
  }

  /// The text without its last 12% of lines.
  private static func shortened(_ text: String) -> String {
    var lines = text.split(separator: "\n", omittingEmptySubsequences: false)
    lines.removeLast(max(1, lines.count / 8))
    return lines.joined(separator: "\n")
  }
}
