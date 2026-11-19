import Foundation

/// Where the voice is: the span of the clip that carries speech and the silences inside it.
struct AvatarValley: Equatable, Sendable {
  var start: Double
  var end: Double
  var duration: Double { end - start }
}

/// The voiced span of a clip: from the first voiced frame to the end of the last, with the
/// silences of at least `minValley` inside it.
struct AvatarVoiced: Equatable, Sendable {
  var start: Double
  var end: Double
  var valleys: [AvatarValley]

  /// A silence shorter than this is a stop closure or a breath of the envelope, not a boundary
  /// between words (seconds).
  private static let minValley = 0.06
  /// The voice threshold sits this far under the reference level, within these bounds (dB).
  private static let belowReferenceDb = 30.0
  private static let thresholdRange = (-55.0, -35.0)

  init?(envelope env: AvatarEnvelope) {
    let threshold = min(
      max(env.referenceDb - Self.belowReferenceDb, Self.thresholdRange.0), Self.thresholdRange.1)
    guard env.isUsable else { return nil }
    let voiced = env.db.indices.map { env.level($0) > threshold }
    guard let first = voiced.firstIndex(of: true), let last = voiced.lastIndex(of: true) else {
      return nil
    }
    var valleys: [AvatarValley] = []
    var runStart: Int?
    for i in first...last {
      switch (voiced[i], runStart) {
      case (false, nil): runStart = i
      case (true, let s?):
        let valley = AvatarValley(start: Double(s) * env.hop, end: Double(i) * env.hop)
        if valley.duration >= Self.minValley { valleys.append(valley) }
        runStart = nil
      default: break
      }
    }
    start = Double(first) * env.hop
    end = Double(last + 1) * env.hop
    self.valleys = valleys
  }
}

/// A boundary the text predicts, in voiced time, and whether the text has a pause mark there.
struct AvatarBoundary: Sendable {
  var at: Double
  var strong: Bool
}

/// Matching the word boundaries of a text to the silences of the audio.
///
/// The text predicts where each word ends (in proportion to its sounds, in voiced time, that is
/// the time left when the silences are taken out). The audio shows where the silences really are.
/// A monotone matching pairs the boundaries with the silences that are close enough, and the pairs
/// become anchors of a piecewise linear time warp, so a word is stretched to fit between the
/// silences that surround it.
enum AvatarBoundaryMatch {
  /// How much a pause mark is worth when it meets a silence, beside the closeness score of 0...1.
  private static let strongBonus = 0.5
  /// The nearest a boundary must be to a silence to be paired with it (seconds, at least).
  private static let minTolerance = 0.12
  /// ... and as a share of the mean word duration.
  private static let toleranceShare = 0.35

  /// A best score and the candidate that gave it.
  private typealias Best = (score: Double, from: Int?)

  /// A Fenwick tree of running maxima over silence indices: the best chain that ends before index
  /// `j`.
  private struct PrefixMax {
    var tree: [Best]

    init(count: Int) { tree = [Best](repeating: (0, nil), count: count + 1) }

    mutating func update(at: Int, best: Best) {
      var k = at + 1
      while k < tree.count {
        if best.score > tree[k].score { tree[k] = best }
        k += k & -k
      }
    }

    /// The best over the indices `0..<at`.
    func query(_ at: Int) -> Best {
      var (k, out): (Int, Best) = (at, (0, nil))
      while k > 0 {
        if tree[k].score > out.score { out = tree[k] }
        k -= k & -k
      }
      return out
    }
  }

  /// Pair boundaries with silences (`positions`, in voiced time), both increasing. The result
  /// lists `(boundary index, silence index)` in order. `meanWord` is the mean duration of a word.
  static func match(boundaries: [AvatarBoundary], positions: [Double], meanWord: Double) -> [(
    Int, Int
  )] {
    let tolerance = avatarMax(minTolerance, toleranceShare * meanWord)
    // (boundary, silence, score), in order of boundary and then silence.
    var candidates: [(Int, Int, Double)] = []
    for (i, b) in boundaries.enumerated() {
      let near = positions.partitionPoint { $0 < b.at - tolerance }
      for j in near..<positions.count {
        let p = positions[j]
        if p > b.at + tolerance { break }
        let bonus = b.strong ? strongBonus : 0.0
        candidates.append((i, j, 1.0 - abs(b.at - p) / tolerance + bonus))
      }
    }
    var chain: [Best] = []
    chain.reserveCapacity(candidates.count)
    var tree = PrefixMax(count: positions.count)
    var groupStart = 0
    while groupStart < candidates.count {
      let boundary = candidates[groupStart].0
      let groupEnd =
        candidates[groupStart...].firstIndex { $0.0 != boundary } ?? candidates.count
      // A chain through this boundary extends one that ended at an earlier boundary and an
      // earlier silence: the tree holds only the earlier boundaries until the group is done.
      for k in groupStart..<groupEnd {
        let (before, parent) = tree.query(candidates[k].1)
        chain.append((before + candidates[k].2, parent))
      }
      for k in groupStart..<groupEnd {
        tree.update(at: candidates[k].1, best: (chain[k].score, k))
      }
      groupStart = groupEnd
    }
    // The last of the best chains, as Rust's `max_by` takes it.
    var at: Int?
    for (k, entry) in chain.enumerated() where at == nil || entry.score >= chain[at ?? 0].score {
      at = k
    }
    var pairs: [(Int, Int)] = []
    while let k = at {
      pairs.append((candidates[k].0, candidates[k].1))
      at = chain[k].from
    }
    return pairs.reversed()
  }
}

/// A monotone piecewise linear map through knots `(from, to)`, increasing in both, with the ends
/// fixed by the first and last knot.
struct AvatarTimeWarp: Sendable {
  private var knots: [(Double, Double)]

  init(knots all: [(Double, Double)]) {
    var kept: [(Double, Double)] = []
    for knot in all {
      if let last = kept.last, knot.0 <= last.0 + 1e-9 || knot.1 <= last.1 + 1e-9 { continue }
      kept.append(knot)
    }
    knots = kept
  }

  func apply(_ x: Double) -> Double {
    let k = knots.partitionPoint { $0.0 <= x }
    let (a, b) = (k > 0 ? knots[k - 1] : nil, k < knots.count ? knots[k] : nil)
    switch (a, b) {
    case (let a?, let b?): return a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
    case (let a?, nil): return a.1 + (x - a.0)
    case (nil, let b?): return b.1 + (x - b.0)
    case (nil, nil): return x
    }
  }
}

extension Array {
  /// The count of leading elements for which `holds` is true, in an array where they come first.
  func partitionPoint(_ holds: (Element) -> Bool) -> Int {
    var (low, high) = (0, count)
    while low < high {
      let mid = (low + high) / 2
      if holds(self[mid]) { low = mid + 1 } else { high = mid }
    }
    return low
  }
}
