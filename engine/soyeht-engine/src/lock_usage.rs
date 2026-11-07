//! What the Core lock does in a minute: for each purpose (a request counts as one purpose of its
//! own, named by its method), how many holds, how much of the minute the lock was held, how long
//! the holds and the waits for the lock were at the median and at the 99th percentile, and where the
//! time went (ADR 0147, step 0). `engine.core_lock_slow` only sees holds of 100 ms or more; the cost
//! of a busy engine is in holds of a few milliseconds, which only an aggregate shows.

use soyeht_platform::{hold_cost::Costs, log::Line};
use std::{collections::BTreeMap, time::Duration};

/// How many purposes a minute writes: the busiest ones.
const LINES_PER_WINDOW: usize = 10;
/// Buckets of the histograms: bucket `b` holds the durations of `2^b` to `2^(b+1)` microseconds,
/// and the last one everything longer (about 16 s).
const BUCKETS: usize = 24;

/// One hold of the lock, as the guard saw it when it ended.
pub(crate) struct Observation {
    pub(crate) purpose: &'static str,
    /// The request method of a `dispatch` hold; empty for the others.
    pub(crate) method: &'static str,
    pub(crate) held: Duration,
    /// From the moment the lock was asked for, so the queue of the blocking pool is in it.
    pub(crate) waited: Duration,
    pub(crate) costs: Costs,
}

#[cfg(test)]
impl Observation {
    /// A hold of `held_ms` for a caller that waited `waited_ms`, with no method and no cost.
    pub(crate) fn for_test(purpose: &'static str, held_ms: u64, waited_ms: u64) -> Self {
        Self {
            purpose,
            method: "",
            held: Duration::from_millis(held_ms),
            waited: Duration::from_millis(waited_ms),
            costs: Costs::default(),
        }
    }
}

fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Default)]
struct Histogram {
    counts: [u64; BUCKETS],
}

impl Histogram {
    fn add(&mut self, micros: u64) {
        let bucket =
            usize::try_from(micros.max(1).ilog2()).map_or(BUCKETS - 1, |b| b.min(BUCKETS - 1));
        self.counts[bucket] += 1;
    }

    /// The upper bound, in microseconds, of the bucket that holds the `permille` quantile.
    fn quantile(&self, permille: u64) -> u64 {
        let total: u64 = self.counts.iter().sum();
        let target = (total * permille).div_ceil(1000).max(1);
        let mut seen = 0;
        for (bucket, count) in self.counts.iter().enumerate() {
            seen += count;
            if seen >= target {
                return 1_u64 << (bucket + 1);
            }
        }
        0
    }
}

#[derive(Default)]
struct Aggregate {
    holds: u64,
    held_us: u64,
    held: Histogram,
    waited: Histogram,
    waited_max_us: u64,
    costs: Costs,
}

/// The holds of a window, by purpose and method.
#[derive(Default)]
pub(crate) struct Usage {
    window_start_ms: Option<u64>,
    by: BTreeMap<(&'static str, &'static str), Aggregate>,
}

impl Usage {
    pub(crate) fn add(&mut self, observation: &Observation, now_ms: u64) {
        self.window_start_ms.get_or_insert(now_ms);
        let aggregate = self
            .by
            .entry((observation.purpose, observation.method))
            .or_default();
        aggregate.holds += 1;
        aggregate.held_us = aggregate.held_us.saturating_add(micros(observation.held));
        aggregate.held.add(micros(observation.held));
        aggregate.waited.add(micros(observation.waited));
        aggregate.waited_max_us = aggregate.waited_max_us.max(micros(observation.waited));
        let costs = &mut aggregate.costs;
        costs.sql = costs.sql.saturating_add(observation.costs.sql);
        costs.commit = costs.commit.saturating_add(observation.costs.commit);
        costs.segment = costs.segment.saturating_add(observation.costs.segment);
        costs.file = costs.file.saturating_add(observation.costs.file);
    }

    /// What the holds of `purpose` (with no method) spent so far in the window, for tests.
    #[cfg(test)]
    pub(crate) fn costs_of(&self, purpose: &str) -> Option<Costs> {
        self.by
            .iter()
            .find(|((name, method), _)| *name == purpose && method.is_empty())
            .map(|(_, aggregate)| aggregate.costs)
    }

    /// The lines of the window when it is over at `now_ms`, and a fresh window.
    pub(crate) fn due(&mut self, now_ms: u64, window_ms: u64) -> Vec<Line> {
        let Some(start) = self.window_start_ms else {
            return Vec::new();
        };
        let elapsed_ms = now_ms.saturating_sub(start);
        if elapsed_ms < window_ms {
            return Vec::new();
        }
        let mut rows: Vec<_> = std::mem::take(&mut self.by).into_iter().collect();
        self.window_start_ms = None;
        rows.sort_by_key(|(_, aggregate)| std::cmp::Reverse(aggregate.held_us));
        rows.into_iter()
            .take(LINES_PER_WINDOW)
            .map(|((purpose, method), aggregate)| {
                usage_line(purpose, method, &aggregate, elapsed_ms)
            })
            .collect()
    }
}

fn usage_line(purpose: &str, method: &str, aggregate: &Aggregate, window_ms: u64) -> Line {
    let busy = aggregate.held_us.saturating_mul(1000) / window_ms.saturating_mul(1000).max(1);
    Line::new("engine.core_lock_usage")
        .field("purpose", purpose)
        .field_opt("method", (!method.is_empty()).then_some(method))
        .field("holds", aggregate.holds)
        .field("busyPermille", busy)
        .field("heldP50Us", aggregate.held.quantile(500))
        .field("heldP99Us", aggregate.held.quantile(990))
        .field("waitP50Us", aggregate.waited.quantile(500))
        .field("waitP99Us", aggregate.waited.quantile(990))
        .field("waitMaxUs", aggregate.waited_max_us)
        .field("sqlMs", millis(aggregate.costs.sql))
        .field("commitMs", millis(aggregate.costs.commit))
        .field("segmentMs", millis(aggregate.costs.segment))
        .field("fileMs", millis(aggregate.costs.file))
        .field("windowMs", window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(purpose: &'static str, held_ms: u64, waited_ms: u64) -> Observation {
        let mut observation = Observation::for_test(purpose, held_ms, waited_ms);
        observation.costs.commit = Duration::from_millis(held_ms / 2);
        observation
    }

    #[test]
    fn quantiles_are_bucket_upper_bounds() {
        let mut histogram = Histogram::default();
        for _ in 0..98 {
            histogram.add(1_000);
        }
        histogram.add(40_000);
        histogram.add(40_000);
        assert_eq!(histogram.quantile(500), 1_024);
        assert_eq!(histogram.quantile(990), 65_536);
    }

    #[test]
    fn a_window_writes_the_busiest_purposes_and_starts_again() {
        let mut usage = Usage::default();
        usage.add(&observation("tick", 4, 1), 0);
        usage.add(&observation("tick", 6, 3), 10);
        usage.add(&observation("offload_apply", 1, 0), 20);
        assert!(usage.due(59_999, 60_000).is_empty());
        let lines = usage.due(60_000, 60_000);
        assert_eq!(lines.len(), 2);
        let first = lines[0].json();
        assert!(first.contains(r#""purpose":"tick""#), "{first}");
        assert!(first.contains(r#""holds":"2""#), "{first}");
        assert!(first.contains(r#""commitMs":"5""#), "{first}");
        assert!(first.contains(r#""waitMaxUs":"3000""#), "{first}");
        assert!(!first.contains("method"), "{first}");
        // The next window is empty until something is held again.
        assert!(usage.due(200_000, 60_000).is_empty());
    }

    #[test]
    fn only_the_busiest_ten_purposes_are_written() {
        let mut usage = Usage::default();
        for (n, method) in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]
            .into_iter()
            .enumerate()
        {
            usage.add(
                &Observation {
                    purpose: "dispatch",
                    method,
                    ..observation("dispatch", n as u64 + 1, 0)
                },
                0,
            );
        }
        let lines = usage.due(60_000, 60_000);
        assert_eq!(lines.len(), LINES_PER_WINDOW);
        assert!(lines[0].json().contains(r#""method":"l""#));
    }
}
