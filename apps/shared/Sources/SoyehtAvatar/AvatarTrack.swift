import Foundation

/// From timed phonemes to a track of mouth targets, and the target at a given time.
///
/// The rules (the lab's animation notes): lead the sound a little, hold the lips for M B P long
/// enough to be seen and start them early, let tongue-only consonants take the shape of their
/// neighbours (coarticulation, the next vowel counting more), skip vowels too short to move the
/// lips, and open wider for stressed and loud vowels.

/// One target of the track, from `t0` until the next target starts (or `t1`).
struct AvatarTarget: Equatable, Sendable {
  var t0: Double
  var t1: Double
  var q: AvatarParams
  var kind: AvatarPhonemeKind
  var p: Unicode.Scalar
}

enum AvatarTrack {
  /// How many targets before the current one may hold a closure that is still closing.
  private static let closureLookback = 32

  /// A loudness gain of a time window relative to the voice.
  typealias Level = (Double, Double) -> Double

  private struct Item {
    var p: Unicode.Scalar
    var t0: Double
    var t1: Double
    var stress: Bool
    var kind: AvatarPhonemeKind
    var q: AvatarParams?
  }

  private static func classify(
    _ p: Unicode.Scalar, duration: Double, stress: Bool, tune: AvatarTune
  ) -> AvatarPhonemeKind {
    if AvatarPhonemes.vowel(p) != nil {
      if AvatarPhonemes.isSemi(p) { return .semi }
      return duration < tune.minVowel && !stress ? .transparent : .vowel
    }
    if AvatarPhonemes.isBilabial(p) { return .bilabial }
    if AvatarPhonemes.isLabiodental(p) { return .labiodental }
    if AvatarPhonemes.isPause(p) { return .pause }
    return .transparent
  }

  private static func vowelTarget(_ item: Item, level: Level, tune: AvatarTune) -> AvatarParams? {
    guard let v = AvatarPhonemes.vowel(item.p) else { return nil }
    let gain = (item.stress ? 1.2 : 1.0) * min(max(level(item.t0, item.t1), 0.5), 1.2)
    // A little exaggeration: short vowels never reach their target, so the opening and the
    // spread are pushed past the plain value.
    var q = AvatarParams(
      o: avatarClamp01(v[0] * gain * tune.exaggerate),
      w: avatarClamp01(0.5 + (v[1] - 0.5) * 1.15),
      r: avatarClamp01(v[2] * 1.1), c: 0, f: 0)
    if item.kind == .semi { q.o *= 0.85 }
    return q
  }

  /// For each item, the target of the nearest vowel after it, not crossing a pause.
  private static func nextVowels(_ items: [Item]) -> [AvatarParams?] {
    var out = [AvatarParams?](repeating: nil, count: items.count)
    var next: AvatarParams?
    for index in items.indices.reversed() {
      out[index] = next
      if items[index].kind == .pause {
        next = nil
      } else if let q = items[index].q {
        next = q
      }
    }
    return out
  }

  /// The target of a tongue or throat consonant: the lips follow the neighbouring vowels.
  private static func consonantTarget(
    _ items: [Item], index: Int, next: AvatarParams?, tune: AvatarTune
  ) -> AvatarParams {
    let before: AvatarParams? =
      index > 0 && items[index - 1].kind != .pause ? items[index - 1].q : nil
    let after = next ?? before
    let near = before ?? after
    let p = items[index].p
    var q: AvatarParams
    if let near {
      // The next vowel counts more: the lips anticipate it.
      let mix = { (pick: (AvatarParams) -> Double) -> Double in
        if let a = before, let b = after { return 0.35 * pick(a) + 0.65 * pick(b) }
        return pick(near)
      }
      q = AvatarParams(o: mix { $0.o }, w: mix { $0.w }, r: mix { $0.r }, c: 0, f: 0)
    } else {
      q = AvatarParams(o: 0.2, w: 0.5, r: 0, c: 0, f: 0)
    }
    if AvatarPhonemes.isSibilant(p) {
      q.o *= 0.45
      q.w = avatarMax(q.w, 0.7)
      q.r *= 0.5
    } else if AvatarPhonemes.isPalatal(p) {
      q.o *= 0.6
      q.r = avatarMin(q.r + 0.25, 1.0)
      q.w = avatarMin(q.w, 0.6)
    } else if "ldtn".unicodeScalars.contains(p) {
      q.o = avatarMax(q.o * 0.75, 0.3)
    } else {
      q.o *= 0.6
    }
    q.o = avatarMax(q.o, tune.floor)
    return q
  }

  /// Build the target track of a clip from its timed phonemes. `level` scales the vowel openings
  /// by the loudness of the voice in the vowel's time window.
  static func build(_ phonemes: [AvatarPhoneme], level: Level, tune: AvatarTune) -> [AvatarTarget] {
    var items: [Item] = []
    items.reserveCapacity(phonemes.count)
    var stress = false
    for ph in phonemes {
      if ph.p == AvatarPhonemes.stress {
        stress = true
        continue
      }
      // A phoneme with no time (not a number, or ending before it starts) is not heard.
      if AvatarPhonemes.isSkipped(ph.p) || !ph.t0.isFinite || !ph.t1.isFinite || ph.t1 < ph.t0 {
        continue
      }
      let kind = classify(ph.p, duration: ph.t1 - ph.t0, stress: stress, tune: tune)
      items.append(Item(p: ph.p, t0: ph.t0, t1: ph.t1, stress: stress, kind: kind, q: nil))
      // The stress is spent on the first full vowel after the mark.
      if kind == .vowel { stress = false }
    }
    for index in items.indices where items[index].kind == .vowel || items[index].kind == .semi {
      items[index].q = vowelTarget(items[index], level: level, tune: tune)
    }
    let next = nextVowels(items)
    for index in items.indices where items[index].q == nil {
      items[index].q =
        switch items[index].kind {
        case .bilabial: AvatarParams(o: 0, w: 0.45, r: 0, c: 1, f: 0)
        case .labiodental: AvatarParams(o: 0.15, w: 0.7, r: 0, c: 0, f: 1)
        case .pause: AvatarParams(o: 0, w: 0.5, r: 0, c: tune.pauseClosure, f: 0)
        default: consonantTarget(items, index: index, next: next[index], tune: tune)
        }
    }
    let targets: [AvatarTarget] = items.map { item in
      var (t0, t1) = (item.t0, item.t1)
      if item.kind == .bilabial || item.kind == .labiodental {
        // Closures are early and held long enough to be seen.
        t0 -= tune.closureLead
        t1 = avatarMax(t1, t0 + tune.minClosure)
      }
      return AvatarTarget(t0: t0, t1: t1, q: item.q ?? .rest, kind: item.kind, p: item.p)
    }
    // A stable sort by start: equal starts keep the order of the phonemes.
    return targets.enumerated().sorted { a, b in
      a.element.t0 != b.element.t0 ? a.element.t0 < b.element.t0 : a.offset < b.offset
    }.map(\.element)
  }

  /// The target at clip time `t`: the last target whose start is at or before `t`, held until the
  /// next one starts. `nil` means nothing is being said now (silence, or outside the clip).
  static func target(in track: [AvatarTarget], at t: Double) -> AvatarParams? {
    let low = track.partitionPoint { $0.t0 <= t }
    guard low > 0 else { return nil }
    let found = low - 1
    // A closure that overlaps the next start keeps priority until its own end. Speech has a
    // dozen phonemes in 0.2 s; a track with thousands there is not speech, and is not searched.
    for k in (0..<found).reversed().prefix(closureLookback) {
      if track[found].t0 - track[k].t0 >= 0.2 { break }
      if track[k].kind == .bilabial || track[k].kind == .labiodental, t < track[k].t1 {
        return track[k].q
      }
    }
    let item = track[found]
    return t < item.t1 + 0.12 ? item.q : nil
  }
}
