//! From timed phonemes to a track of mouth targets, and the target at a given time.
//!
//! The rules (the lab's animation notes): lead the sound a little, hold the lips for M B P long
//! enough to be seen and start them early, let tongue-only consonants take the shape of their
//! neighbours (coarticulation, the next vowel counting more), skip vowels too short to move the
//! lips, and open wider for stressed and loud vowels. Ported from `buildTrack` and `targetAt` of
//! `rig/runtime/mouth.js` (Apache-2.0, the same authors).

use crate::params::{Params, clamp01};
use crate::phoneme::{
    Kind, Phoneme, STRESS, is_bilabial, is_labiodental, is_palatal, is_pause, is_semi, is_sibilant,
    is_skipped, vowel,
};
use crate::tune::Tune;

/// One target of the track, from `t0` until the next target starts (or `t1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    pub t0: f64,
    pub t1: f64,
    pub q: Params,
    pub kind: Kind,
    pub p: char,
}

/// A loudness gain of a time window relative to the voice: about 0.55 (soft) to 1.1 (loud).
pub type Level<'a> = &'a dyn Fn(f64, f64) -> f64;

struct Item {
    p: char,
    t0: f64,
    t1: f64,
    stress: bool,
    kind: Kind,
    q: Option<Params>,
}

fn classify(p: char, duration: f64, stress: bool, tune: &Tune) -> Kind {
    if vowel(p).is_some() {
        if is_semi(p) {
            Kind::Semi
        } else if duration < tune.min_vowel && !stress {
            Kind::Transparent
        } else {
            Kind::Vowel
        }
    } else if is_bilabial(p) {
        Kind::Bilabial
    } else if is_labiodental(p) {
        Kind::Labiodental
    } else if is_pause(p) {
        Kind::Pause
    } else {
        Kind::Transparent
    }
}

fn vowel_target(item: &Item, level: Level, tune: &Tune) -> Option<Params> {
    let [o, w, r] = vowel(item.p)?;
    let gain = (if item.stress { 1.2 } else { 1.0 }) * level(item.t0, item.t1).clamp(0.5, 1.2);
    // A little exaggeration: short vowels never reach their target, so the opening and the
    // spread are pushed past the plain value.
    let mut q = Params::new(
        clamp01(o * gain * tune.exaggerate),
        clamp01(0.5 + (w - 0.5) * 1.15),
        clamp01(r * 1.1),
        0.0,
        0.0,
    );
    if item.kind == Kind::Semi {
        q.o *= 0.85;
    }
    Some(q)
}

/// The nearest vowel target on one side of `index`, not crossing a pause.
fn neighbour(items: &[Item], index: usize, forward: bool) -> Option<Params> {
    let mut k = index;
    loop {
        k = if forward { k + 1 } else { k.checked_sub(1)? };
        let item = items.get(k)?;
        if item.kind == Kind::Pause {
            return None;
        }
        if let Some(q) = item.q {
            return Some(q);
        }
    }
}

fn consonant_target(items: &[Item], index: usize, tune: &Tune) -> Params {
    let before = neighbour(items, index, false);
    let after = neighbour(items, index, true).or(before);
    let near = before.or(after);
    let p = items[index].p;
    let mut q = match near {
        None => Params::new(0.2, 0.5, 0.0, 0.0, 0.0),
        Some(c) => {
            // The next vowel counts more: the lips anticipate it.
            let mix = |pick: fn(&Params) -> f64| match (before, after) {
                (Some(a), Some(b)) => 0.35 * pick(&a) + 0.65 * pick(&b),
                _ => pick(&c),
            };
            Params::new(mix(|q| q.o), mix(|q| q.w), mix(|q| q.r), 0.0, 0.0)
        }
    };
    if is_sibilant(p) {
        q.o *= 0.45;
        q.w = q.w.max(0.7);
        q.r *= 0.5;
    } else if is_palatal(p) {
        q.o *= 0.6;
        q.r = (q.r + 0.25).min(1.0);
        q.w = q.w.min(0.6);
    } else if matches!(p, 'l' | 'd' | 't' | 'n') {
        q.o = (q.o * 0.75).max(0.3);
    } else {
        q.o *= 0.6;
    }
    q.o = q.o.max(tune.floor);
    q
}

/// Build the target track of a clip from its timed phonemes. `level` scales the vowel openings
/// by the loudness of the voice in the vowel's time window.
pub fn build_track(phonemes: &[Phoneme], level: Level, tune: &Tune) -> Vec<Target> {
    let mut items: Vec<Item> = Vec::with_capacity(phonemes.len());
    let mut stress = false;
    for ph in phonemes {
        if ph.p == STRESS {
            stress = true;
            continue;
        }
        if is_skipped(ph.p) {
            continue;
        }
        let kind = classify(ph.p, ph.t1 - ph.t0, stress, tune);
        items.push(Item {
            p: ph.p,
            t0: ph.t0,
            t1: ph.t1,
            stress,
            kind,
            q: None,
        });
        // The stress is spent on the first full vowel after the mark.
        if kind == Kind::Vowel {
            stress = false;
        }
    }
    for item in items.iter_mut() {
        if matches!(item.kind, Kind::Vowel | Kind::Semi) {
            item.q = vowel_target(item, level, tune);
        }
    }
    for i in 0..items.len() {
        if items[i].q.is_some() {
            continue;
        }
        let q = match items[i].kind {
            Kind::Bilabial => Params::new(0.0, 0.45, 0.0, 1.0, 0.0),
            Kind::Labiodental => Params::new(0.15, 0.7, 0.0, 0.0, 1.0),
            Kind::Pause => Params::new(0.0, 0.5, 0.0, tune.pause_closure, 0.0),
            _ => consonant_target(&items, i, tune),
        };
        items[i].q = Some(q);
    }
    let mut track: Vec<Target> = items
        .iter()
        .map(|it| {
            let (mut t0, mut t1) = (it.t0, it.t1);
            if matches!(it.kind, Kind::Bilabial | Kind::Labiodental) {
                // Closures are early and held long enough to be seen.
                t0 -= tune.closure_lead;
                t1 = t1.max(t0 + tune.min_closure);
            }
            Target {
                t0,
                t1,
                q: it.q.unwrap_or(Params::REST),
                kind: it.kind,
                p: it.p,
            }
        })
        .collect();
    track.sort_by(|a, b| a.t0.total_cmp(&b.t0));
    track
}

/// The target at clip time `t`: the last target whose start is at or before `t`, held until the
/// next one starts. `None` means nothing is being said now (silence, or outside the clip).
pub fn target_at(track: &[Target], t: f64) -> Option<Params> {
    let found = track.partition_point(|it| it.t0 <= t).checked_sub(1)?;
    // A closure that overlaps the next start keeps priority until its own end.
    for k in (0..found).rev() {
        if track[found].t0 - track[k].t0 >= 0.2 {
            break;
        }
        if matches!(track[k].kind, Kind::Bilabial | Kind::Labiodental) && t < track[k].t1 {
            return Some(track[k].q);
        }
    }
    let item = &track[found];
    (t < item.t1 + 0.12).then_some(item.q)
}
