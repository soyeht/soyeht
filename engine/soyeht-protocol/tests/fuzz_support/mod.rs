//! The harness of the fuzz tests (docs/security-testing.md): seeded, deterministic mutation
//! fuzzing that runs in the normal test suite. Every crate with a fuzz test includes this file.
//!
//! A run has a budget. By default it is a fixed number of cases from a fixed seed, so the gate
//! runs the same cases every time and finishes in seconds. `SOYEHT_FUZZ_SECONDS` runs each target
//! for that long instead, from a fresh seed (or `SOYEHT_FUZZ_SEED`, a decimal number). A failing
//! case is minimized and printed as hex, with the seed, so it can be replayed and kept as a test.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Map, Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The longest a single case may run before the watchdog ends the process: a hang is a failure.
const CASE_DEADLINE: Duration = Duration::from_secs(20);

/// A splitmix64 generator: small, fast, and the same sequence on every system. The retention
/// tests of `soyeht-ptyd` use it too.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `limit` (0 when `limit` is 0).
    pub fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            0
        } else {
            (self.next_u64() % limit as u64) as usize
        }
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    pub fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| self.next_u64() as u8).collect()
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// How many cases a target runs, and from which seed.
struct Budget {
    iterations: u64,
    deadline: Option<Instant>,
    seed: u64,
}

fn name_seed(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01B3)
    })
}

fn budget(name: &str, default_iterations: u64) -> Budget {
    let seconds = std::env::var("SOYEHT_FUZZ_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let explicit = std::env::var("SOYEHT_FUZZ_SEED")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    let seed = explicit.unwrap_or_else(|| match seconds {
        Some(_) => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |elapsed| elapsed.as_nanos() as u64),
        None => name_seed(name),
    });
    Budget {
        iterations: if seconds.is_some() {
            u64::MAX
        } else {
            default_iterations
        },
        deadline: seconds.map(|seconds| Instant::now() + Duration::from_secs(seconds)),
        seed,
    }
}

/// Ends the process, printing the case, when one case runs past [`CASE_DEADLINE`].
fn watchdog(name: String) -> (Arc<AtomicU64>, Arc<Mutex<Vec<u8>>>) {
    let started = Arc::new(AtomicU64::new(0));
    let current = Arc::new(Mutex::new(Vec::new()));
    let (watch_started, watch_current) = (Arc::clone(&started), Arc::clone(&current));
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let since = watch_started.load(Ordering::Relaxed);
            if since != 0 && now_ms().saturating_sub(since) > CASE_DEADLINE.as_millis() as u64 {
                let input = watch_current.lock().map(|input| input.clone());
                eprintln!(
                    "fuzz {name}: a case ran longer than {CASE_DEADLINE:?}\ninput: {}",
                    hex(&input.unwrap_or_default())
                );
                std::process::abort();
            }
        }
    });
    (started, current)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |elapsed| elapsed.as_millis() as u64)
        .max(1)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What a byte-fuzz target takes: the seed inputs it starts from, the tokens that steer
/// mutations toward its grammar, and the longest input.
pub struct Corpus<'a> {
    pub seeds: &'a [&'a [u8]],
    pub tokens: &'a [&'a [u8]],
    pub max_len: usize,
}

const EDGE_BYTES: [u8; 12] = [
    0x00, 0x01, 0x7f, 0x80, 0xfe, 0xff, b'\n', b'"', b'{', b'}', b'\\', b'/',
];

fn mutate(rng: &mut Rng, corpus: &Corpus<'_>) -> Vec<u8> {
    if corpus.seeds.is_empty() || rng.chance(8) {
        let length = rng.below(corpus.max_len.min(96) + 1);
        return rng.bytes(length);
    }
    let mut input = rng.pick(corpus.seeds).to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(9) {
            0 if !input.is_empty() => {
                let at = rng.below(input.len());
                input[at] ^= 1 << rng.below(8);
            }
            1 if !input.is_empty() => {
                let at = rng.below(input.len());
                input[at] = *rng.pick(&EDGE_BYTES);
            }
            2 => {
                let at = rng.below(input.len() + 1);
                let length = 1 + rng.below(8);
                let extra = rng.bytes(length);
                input.splice(at..at, extra);
            }
            3 if !input.is_empty() => {
                let from = rng.below(input.len());
                let to = (from + 1 + rng.below(8)).min(input.len());
                input.drain(from..to);
            }
            4 if !input.is_empty() => {
                let from = rng.below(input.len());
                let to = (from + 1 + rng.below(16)).min(input.len());
                let copy = input[from..to].to_vec();
                let at = rng.below(input.len() + 1);
                input.splice(at..at, copy);
            }
            5 if !corpus.tokens.is_empty() => {
                let token = rng.pick(corpus.tokens);
                let at = rng.below(input.len() + 1);
                if rng.chance(2) {
                    input.splice(at..at, token.iter().copied());
                } else {
                    let to = (at + token.len()).min(input.len());
                    input.splice(at..to, token.iter().copied());
                }
            }
            6 => {
                let keep = rng.below(input.len() + 1);
                input.truncate(keep);
            }
            7 => {
                let other = rng.pick(corpus.seeds);
                let from = rng.below(other.len() + 1);
                let at = rng.below(input.len() + 1);
                input.splice(at.., other[from..].iter().copied());
            }
            _ if input.len() >= 4 => {
                // An integer field set to a boundary value.
                let at = rng.below(input.len() - 3);
                let edge = *rng.pick(&[0u32, 1, 0x7fff_ffff, 0x8000_0000, u32::MAX, 0x0010_0000]);
                input[at..at + 4].copy_from_slice(&edge.to_be_bytes());
            }
            _ => {}
        }
    }
    input.truncate(corpus.max_len);
    input
}

fn fails(target: &dyn Fn(&[u8]), input: &[u8]) -> bool {
    catch_unwind(AssertUnwindSafe(|| target(input))).is_err()
}

/// The shortest input found by dropping chunks, then bytes, while `target` still panics.
fn minimize(target: &dyn Fn(&[u8]), input: &[u8]) -> Vec<u8> {
    let mut best = input.to_vec();
    let mut chunk = best.len().div_ceil(2).max(1);
    while chunk >= 1 {
        let mut at = 0;
        while at < best.len() {
            let mut candidate = best.clone();
            candidate.drain(at..(at + chunk).min(best.len()));
            if fails(target, &candidate) {
                best = candidate;
            } else {
                at += chunk;
            }
        }
        if chunk == 1 {
            break;
        }
        chunk = chunk.div_ceil(2);
    }
    best
}

/// Runs `target` on mutations of the corpus. A panic fails the test with the minimized input.
pub fn fuzz_bytes(
    name: &str,
    default_iterations: u64,
    corpus: &Corpus<'_>,
    target: impl Fn(&[u8]),
) {
    let budget = budget(name, default_iterations);
    let mut rng = Rng::new(budget.seed);
    let (started, current) = watchdog(name.to_owned());
    // The seeds themselves are cases too, so a regression on a known input fails at once.
    let mut cases: u64 = 0;
    while cases < budget.iterations && budget.deadline.is_none_or(|end| Instant::now() < end) {
        let input = match corpus.seeds.get(cases as usize) {
            Some(seed) => seed.to_vec(),
            None => mutate(&mut rng, corpus),
        };
        if let Ok(mut slot) = current.lock() {
            slot.clone_from(&input);
        }
        started.store(now_ms(), Ordering::Relaxed);
        let failed = fails(&target, &input);
        started.store(0, Ordering::Relaxed);
        if failed {
            let small = minimize(&target, &input);
            panic!(
                "fuzz {name}: panic after {cases} cases (seed {})\nminimized input ({} bytes): {}",
                budget.seed,
                small.len(),
                hex(&small)
            );
        }
        cases += 1;
    }
    eprintln!("fuzz {name}: {cases} cases, seed {}", budget.seed);
}

/// Text targets: the corpus is UTF-8 and so is every case (a lossy conversion of the mutation).
pub fn fuzz_text(name: &str, default_iterations: u64, corpus: &Corpus<'_>, target: impl Fn(&str)) {
    fuzz_bytes(name, default_iterations, corpus, |bytes| {
        target(&String::from_utf8_lossy(bytes));
    });
}

/// Values a JSON field is replaced with: another type, a boundary number, a hostile string.
fn interesting(rng: &mut Rng) -> Value {
    let long = "a".repeat(1 + rng.below(70_000));
    match rng.below(16) {
        0 => Value::Null,
        1 => json!(true),
        2 => json!(0),
        3 => json!(-1),
        4 => json!(u64::MAX),
        5 => json!(i64::MIN),
        6 => json!(1.5e300),
        7 => json!(""),
        8 => json!(long),
        9 => json!("../../etc/passwd"),
        10 => json!("\u{0}"),
        11 => json!("file:///etc/passwd"),
        12 => json!([]),
        13 => json!({}),
        14 => json!("\u{202e}\u{feff}\u{1f600}"),
        _ => json!(4_294_967_296u64),
    }
}

/// Changes one place of `value`: a leaf replaced, a key dropped or added, a list grown.
pub fn mutate_json(rng: &mut Rng, value: &mut Value) {
    match value {
        Value::Object(object) if !object.is_empty() && !rng.chance(4) => {
            let keys: Vec<String> = object.keys().cloned().collect();
            let key = rng.pick(&keys).clone();
            match rng.below(5) {
                0 => {
                    object.remove(&key);
                }
                1 => {
                    object.insert(format!("{key}x"), interesting(rng));
                }
                2 => {
                    object.insert(key, interesting(rng));
                }
                _ => {
                    if let Some(inner) = object.get_mut(&key) {
                        mutate_json(rng, inner);
                    }
                }
            }
        }
        Value::Array(items) if !items.is_empty() && !rng.chance(4) => {
            let at = rng.below(items.len());
            match rng.below(4) {
                0 => {
                    items.remove(at);
                }
                1 => items.push(items[at].clone()),
                _ => mutate_json(rng, &mut items[at]),
            }
        }
        Value::Object(object) => {
            object.insert("extra".to_owned(), interesting(rng));
        }
        _ => *value = interesting(rng),
    }
}

/// Runs `target` on `rounds` of JSON mutations of each seed value.
pub fn fuzz_json(name: &str, default_iterations: u64, seeds: &[Value], target: impl Fn(&Value)) {
    let budget = budget(name, default_iterations);
    let mut rng = Rng::new(budget.seed);
    let (started, current) = watchdog(name.to_owned());
    let mut cases: u64 = 0;
    while cases < budget.iterations && budget.deadline.is_none_or(|end| Instant::now() < end) {
        let mut value = seeds[rng.below(seeds.len())].clone();
        if cases >= seeds.len() as u64 {
            for _ in 0..=rng.below(3) {
                mutate_json(&mut rng, &mut value);
            }
        }
        if let Ok(mut slot) = current.lock() {
            *slot = value.to_string().into_bytes();
        }
        started.store(now_ms(), Ordering::Relaxed);
        let failed = catch_unwind(AssertUnwindSafe(|| target(&value))).is_err();
        started.store(0, Ordering::Relaxed);
        if failed {
            panic!(
                "fuzz {name}: panic after {cases} cases (seed {})\ninput: {value}",
                budget.seed
            );
        }
        cases += 1;
    }
    eprintln!("fuzz {name}: {cases} cases, seed {}", budget.seed);
}

/// A value that satisfies `rule` of the contract's schema, as small as the rule allows: the
/// seed of a JSON fuzz, so a mutation starts one step from valid.
pub fn example(schema: &Value, rule: &Value, depth: usize) -> Value {
    if let Some(name) = rule
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        return if depth > 12 {
            Value::Null
        } else {
            example(schema, &schema["$defs"][name], depth + 1)
        };
    }
    if let Some(first) = rule
        .get("oneOf")
        .and_then(Value::as_array)
        .and_then(|branches| branches.first())
    {
        return example(schema, first, depth + 1);
    }
    if let Some(first) = rule
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|options| options.first())
    {
        return first.clone();
    }
    match rule.get("type").and_then(Value::as_str) {
        Some("boolean") => json!(false),
        Some("integer" | "number") => {
            json!(rule.get("minimum").and_then(Value::as_i64).unwrap_or(0))
        }
        Some("array") => json!([]),
        Some("string") => {
            if rule.get("pattern").is_some() {
                return json!("0");
            }
            if rule.get("contentEncoding").and_then(Value::as_str) == Some("base64") {
                return json!("AAAA");
            }
            let length = rule.get("minLength").and_then(Value::as_u64).unwrap_or(1) as usize;
            json!("a".repeat(length.max(1)))
        }
        Some("object") => {
            let mut object = Map::new();
            let required = rule["required"].as_array().cloned().unwrap_or_default();
            for key in required.iter().filter_map(Value::as_str) {
                let property = &rule["properties"][key];
                object.insert(key.to_owned(), example(schema, property, depth + 1));
            }
            Value::Object(object)
        }
        _ => Value::Null,
    }
}
