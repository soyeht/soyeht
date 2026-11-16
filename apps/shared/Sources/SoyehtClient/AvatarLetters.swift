import Foundation

/// The language whose letter rules read the text.
enum AvatarLanguage: Sendable {
  case portugueseBrazil
  case english

  /// The id a vector file or a voice setting names the language by (`en`; anything else is
  /// Portuguese of Brazil, as in the vector files).
  init(id: String) { self = id == "en" ? .english : .portugueseBrazil }
}

/// Letters to phonemes by rules of our own, for a TTS that gives only audio.
///
/// This is a rough track, and it is meant to be: the mouth only needs the lip shape of each sound,
/// and the aligner places the result on the audio that actually plays. Portuguese (pt-BR) follows
/// the lab's `phonemesFromWords`, with the accented vowel marked as the stressed one. English is a
/// smaller table of the same kind. No external phonemizer is used: the usual one is GPL (ADR 0166,
/// licenses). The text is read by Unicode scalars, as the Rust crate reads it.
enum AvatarLetters {
  /// One piece of a text: a word, or a pause mark.
  enum Token: Equatable {
    case word(String)
    case pause(Unicode.Scalar)
  }

  /// Letters and digits (Unicode Alphabetic, or number categories Nd, Nl and No).
  static func isAlphanumeric(_ c: Unicode.Scalar) -> Bool {
    c.properties.isAlphabetic
      || [.decimalNumber, .letterNumber, .otherNumber].contains(c.properties.generalCategory)
  }

  /// Symbols that are said (and so have a syllable), not dropped like quotes and brackets.
  private static func isSymbol(_ c: Unicode.Scalar) -> Bool {
    "%$€£¥+=/<>@&§°×÷^".unicodeScalars.contains(c)
  }

  /// A letter, digit, symbol or letter of another script that the letter rules do not read: it is
  /// said, so it gets a neutral syllable, and the mouth moves for it.
  private static func isUnreadable(_ c: Unicode.Scalar) -> Bool {
    isAlphanumeric(c) || isSymbol(c)
  }

  /// Split a text into words and pause marks. A word is a run of letters and digits; a symbol
  /// that is said (`%`, `+`, `/` ...) is a word of its own; everything else separates words and is
  /// dropped.
  static func tokens(_ text: String) -> [Token] {
    var out: [Token] = []
    var word = String.UnicodeScalarView()
    for c in text.unicodeScalars {
      if isAlphanumeric(c) {
        word.append(c)
        continue
      }
      if !word.isEmpty {
        out.append(.word(String(word)))
        word = String.UnicodeScalarView()
      }
      if AvatarPhonemes.isPause(c) && c != "-" {
        out.append(.pause(c))
      } else if isSymbol(c) {
        out.append(.word(String(c)))
      }
    }
    if !word.isEmpty { out.append(.word(String(word))) }
    return out
  }

  /// A phoneme and whether the letter that gave it carries the written accent.
  private typealias Seq = [(Unicode.Scalar, Bool)]

  /// The phonemes of one word, with the stress mark `ˈ` placed before the stressed vowel: the
  /// accented one when the word has an accent, else the first.
  static func phonemize(_ word: String, language: AvatarLanguage) -> [Unicode.Scalar] {
    let letters = Array(word.lowercased().unicodeScalars)
    let seq =
      switch language {
      case .portugueseBrazil: portuguese(letters)
      case .english: english(letters)
      }
    let isVowel = { (p: Unicode.Scalar) in
      AvatarPhonemes.vowel(p) != nil && !AvatarPhonemes.isSemi(p)
    }
    let stressed =
      seq.firstIndex { $0.1 && isVowel($0.0) } ?? seq.firstIndex { isVowel($0.0) }
    var out: [Unicode.Scalar] = []
    out.reserveCapacity(seq.count + 1)
    for (i, item) in seq.enumerated() {
      if i == stressed { out.append(AvatarPhonemes.stress) }
      out.append(item.0)
    }
    return out
  }

  /// Two letters that make one sound (or a fixed pair of sounds).
  private typealias Digraphs = [(String, String)]

  private static let portugueseDigraphs: Digraphs = [
    ("ch", "ʃ"), ("lh", "ʎ"), ("nh", "ɲ"), ("rr", "x"), ("ss", "s"), ("qu", "k"),
  ]

  private static let englishDigraphs: Digraphs = [
    ("th", "θ"), ("sh", "ʃ"), ("ch", "ʧ"), ("ph", "f"), ("ck", "k"), ("qu", "kw"), ("wh", "w"),
    ("ng", "n"), ("ee", "i"), ("ea", "i"), ("oo", "u"), ("oa", "o"), ("ou", "ɑʊ"), ("ow", "ɑʊ"),
    ("ai", "eɪ"), ("ay", "eɪ"), ("oi", "ɔɪ"), ("oy", "ɔɪ"),
  ]

  private static func digraph(
    _ table: Digraphs, _ a: Unicode.Scalar, _ b: Unicode.Scalar?
  ) -> String? {
    guard let b else { return nil }
    return table.first { Array($0.0.unicodeScalars) == [a, b] }?.1
  }

  private static func portugueseFold(_ c: Unicode.Scalar) -> Unicode.Scalar? {
    switch c {
    case "á", "à", "â": "a"
    case "ã": "ɐ"
    case "é": "ɛ"
    case "ê": "e"
    case "í": "i"
    case "ó": "ɔ"
    case "ô", "õ": "o"
    case "ú", "ü": "u"
    default: nil
    }
  }

  /// Where the walk over a word is: the letter and the two after it.
  private struct At {
    var i: Int
    var c: Unicode.Scalar
    var next: Unicode.Scalar?
    var after: Unicode.Scalar?
  }

  /// Walk over the letters of a word: a digraph of the table makes its sounds, and any other
  /// letter is handed to `one`, which pushes its sounds and returns how many further letters it
  /// used.
  private static func walk(
    _ chars: [Unicode.Scalar], _ digraphs: Digraphs, _ one: (inout Seq, At) -> Int
  ) -> Seq {
    var seq: Seq = []
    var i = 0
    while i < chars.count {
      let next = i + 1 < chars.count ? chars[i + 1] : nil
      if let sound = digraph(digraphs, chars[i], next) {
        seq.append(contentsOf: sound.unicodeScalars.map { ($0, false) })
        i += 2
        continue
      }
      let at = At(i: i, c: chars[i], next: next, after: i + 2 < chars.count ? chars[i + 2] : nil)
      i += 1 + one(&seq, at)
    }
    return seq
  }

  /// A neutral syllable: a tongue consonant and a neutral vowel.
  private static func neutral(_ seq: inout Seq) {
    seq.append(contentsOf: [("d", false), ("ə", false)])
  }

  private static func portuguese(_ chars: [Unicode.Scalar]) -> Seq {
    let last = max(chars.count - 1, 0)
    let front = { (c: Unicode.Scalar?) in
      guard let c else { return false }
      return "eéêií".unicodeScalars.contains(c)
    }
    let vowelLetter = { (c: Unicode.Scalar?) in
      guard let c else { return false }
      return "aeiouáéêíóôúãõ".unicodeScalars.contains(c)
    }
    let accented = "áâéêíóôú".unicodeScalars
    return walk(chars, portugueseDigraphs) { seq, at in
      let (i, c, next, after) = (at.i, at.c, at.next, at.after)
      if c == "g" && next == "u" && front(after) {
        seq.append(("g", false))
        return 1
      }
      switch c {
      case "ç": seq.append(("s", false))
      case "x": seq.append(("ʃ", false))
      case "h": break
      case "c": seq.append((front(next) ? "s" : "k", false))
      case "g": seq.append((front(next) ? "ʒ" : "g", false))
      case "j": seq.append(("ʒ", false))
      case "m", "n":
        // A nasal before a consonant or at the end nasalises the vowel before it and has no
        // lips of its own, except a final m, which closes them.
        if vowelLetter(next) {
          seq.append((c, false))
        } else if c == "m" && i == last && !seq.isEmpty {
          seq.append(("m", false))
        }
      default:
        if let folded = portugueseFold(c) {
          seq.append((folded, accented.contains(c)))
        } else if "aeiou".unicodeScalars.contains(c) {
          let wordEnd = i == last || (i + 2 == chars.count && next == "s")
          let p: Unicode.Scalar =
            switch (wordEnd, c) {
            case (true, "o"): "ʊ"
            case (true, "e"): "ɪ"
            case (true, "a"): "ɐ"
            default: c
            }
          seq.append((p, false))
        } else if c == "w" {
          seq.append(("w", false))
        } else if c == "y" {
          seq.append(("i", false))
        } else if "bpmfvtdnlrszkg".unicodeScalars.contains(c) {
          seq.append((c == "r" ? "ɾ" : c, false))
        } else if isUnreadable(c) {
          neutral(&seq)
        }
      }
      return 0
    }
  }

  private static func english(_ chars: [Unicode.Scalar]) -> Seq {
    walk(chars, englishDigraphs) { seq, at in
      let (i, c, next) = (at.i, at.c, at.next)
      let front = next.map { "eiy".unicodeScalars.contains($0) } ?? false
      let silentFinalE =
        c == "e" && i + 1 == chars.count && chars.count > 3
        && !"aeiouy".unicodeScalars.contains(chars[i - 1])
      if silentFinalE { return 0 }
      switch c {
      case "a": seq.append(("æ", false))
      case "e": seq.append(("ɛ", false))
      case "i": seq.append(("ɪ", false))
      case "o": seq.append(("ɑ", false))
      case "u": seq.append(("ɐ", false))
      case "y": seq.append((i == 0 ? "j" : "i", false))
      case "c": seq.append((front ? "s" : "k", false))
      case "g": seq.append((front ? "ʤ" : "g", false))
      case "j": seq.append(("ʤ", false))
      case "x": seq.append(contentsOf: [("k", false), ("s", false)])
      case "r": seq.append(("ɹ", false))
      default:
        if c.isASCII && ("a"..."z").contains(c) {
          seq.append((c, false))
        } else if isUnreadable(c) {
          neutral(&seq)
        }
      }
      return 0
    }
  }
}
