//! Statistics for the capture cadence measurement: paired ratios, a
//! distribution-free confidence interval for their median, and the p95 rule
//! judged against the host's own noise floor.

pub(super) const CADENCE_MEDIAN_PERCENT_LIMIT: u128 = 5;
pub(super) const CADENCE_TAIL_MICROSECOND_LIMIT: u128 = 5_000;

/// The paired ratio limit, in per-mille of the capture-off time.
const CADENCE_RATIO_LIMIT_PER_MILLE: u128 = 1_000 + CADENCE_MEDIAN_PERCENT_LIMIT * 10;
/// Each side of the median's distribution-free confidence interval may
/// exclude at most this much binomial probability (a 95 percent interval).
const CADENCE_CONFIDENCE_TAIL: f64 = 0.025;

/// The p95 rule is judged over at least this many windows.
const CADENCE_TAIL_WINDOWS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CadenceVerdict {
    WithinLimit,
    OverLimit,
    /// Capture-on measured faster than capture-off beyond the noise, which
    /// only a contaminated measurement can produce.
    Implausible,
    Undecided,
}

/// Steady samples pooled across measurement windows. Each entry of `off`
/// and `on` at the same index was timed in the same interleaved pair, so
/// host load common to the pair cancels out of their ratio.
#[derive(Default)]
pub(super) struct CadencePool {
    pub(super) off: Vec<u128>,
    pub(super) on: Vec<u128>,
    /// Windows judged by the p95 rule, and how many of them broke it.
    pub(super) tail_windows: usize,
    pub(super) tail_violations: usize,
}

impl CadencePool {
    pub(super) fn add_window(&mut self, off: &[u128], on: &[u128]) {
        self.off.extend_from_slice(off);
        self.on.extend_from_slice(on);
        self.tail_windows += 1;
        self.tail_violations += usize::from(!window_tail_within_limit(off, on));
    }

    pub(super) fn tail_resolved(&self) -> bool {
        self.tail_windows >= CADENCE_TAIL_WINDOWS
    }

    /// A capture stall recurs; a host spike that lands on one arm does not.
    /// The tail holds unless most of the judged windows broke the rule.
    pub(super) fn tail_within_limit(&self) -> bool {
        self.tail_windows > 0 && self.tail_violations * 2 <= self.tail_windows
    }

    pub(super) fn paired_ratios_per_mille(&self) -> Vec<u128> {
        let mut ratios = self
            .off
            .iter()
            .zip(&self.on)
            .map(|(off, on)| on.saturating_mul(1_000) / (*off).max(1))
            .collect::<Vec<_>>();
        ratios.sort_unstable();
        ratios
    }
}

/// Order-statistic bounds of a 95 percent confidence interval for the
/// median of `sorted`: the largest `d` with P(Binomial(n, 1/2) <= d) within
/// the tail gives [x(d+1), x(n-d)]. `None` when `n` is too small to bound.
pub(super) fn median_confidence_bounds(sorted: &[u128]) -> Option<(u128, u128)> {
    let n = sorted.len();
    let mut probability = 0.5_f64.powi(i32::try_from(n).ok()?);
    let mut cumulative = probability;
    let mut depth = None;
    for successes in 0..n / 2 {
        if cumulative > CADENCE_CONFIDENCE_TAIL {
            break;
        }
        depth = Some(successes);
        probability *= (n - successes) as f64 / (successes + 1) as f64;
        cumulative += probability;
    }
    depth.map(|depth| (sorted[depth], sorted[n - 1 - depth]))
}

/// Whether capture-on stays within the median overhead limit. The verdict
/// is decided only when the confidence interval of the median paired ratio
/// lies wholly on one side of the limit; host noise widens the interval and
/// asks for another window instead of deciding on a noisy point estimate.
/// After the last window the pooled median itself decides. `ratios_per_mille`
/// must be sorted.
pub(super) fn cadence_verdict(ratios_per_mille: &[u128], last_window: bool) -> CadenceVerdict {
    if !capture_difference_is_plausible(ratios_per_mille) {
        return CadenceVerdict::Implausible;
    }
    if let Some((low, high)) = median_confidence_bounds(ratios_per_mille) {
        if high <= CADENCE_RATIO_LIMIT_PER_MILLE {
            return CadenceVerdict::WithinLimit;
        }
        if low > CADENCE_RATIO_LIMIT_PER_MILLE {
            return CadenceVerdict::OverLimit;
        }
    }
    if !last_window {
        return CadenceVerdict::Undecided;
    }
    if percentile(ratios_per_mille, 1, 2) <= CADENCE_RATIO_LIMIT_PER_MILLE {
        CadenceVerdict::WithinLimit
    } else {
        CadenceVerdict::OverLimit
    }
}

/// The per-window tail rule: capture-on's p95 stays within 5 ms of
/// capture-off's. A comparison cannot resolve less than the host's own
/// noise floor in that window, measured as how far capture-off's p95 sits
/// above its median (capture-off does no capture work, so that spread is
/// pure host noise). On a quiet host the floor is below 5 ms and the
/// original 5 ms rule applies unchanged.
fn window_tail_within_limit(off: &[u128], on: &[u128]) -> bool {
    let off_p95 = percentile(off, 95, 100);
    let noise_floor = off_p95 - percentile(off, 1, 2);
    percentile(on, 95, 100) <= off_p95 + noise_floor.max(CADENCE_TAIL_MICROSECOND_LIMIT)
}

pub(super) fn percentile(values: &[u128], numerator: usize, denominator: usize) -> u128 {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let index = (sorted.len() * numerator).div_ceil(denominator) - 1;
    sorted[index]
}

/// Capture adds work to every poll, so capture-on cannot be faster than
/// capture-off by more than the median limit. The sorted paired ratios
/// show such a speedup only when their median's whole confidence interval
/// lies below 1 - limit; a noisy speedup in one direction is noise, not
/// contamination, and is kept so that it cancels noise in the other.
fn capture_difference_is_plausible(sorted_ratios_per_mille: &[u128]) -> bool {
    median_confidence_bounds(sorted_ratios_per_mille)
        .is_none_or(|(_, high)| high >= 1_000 - CADENCE_MEDIAN_PERCENT_LIMIT * 10)
}

#[test]
fn cadence_sanity_keeps_noise_and_rejects_only_implausible_speedups() {
    // Noise in either direction, including large spikes, is plausible.
    assert!(capture_difference_is_plausible(&[
        700, 900, 980, 1_000, 1_010, 1_400, 2_000
    ]));
    assert!(capture_difference_is_plausible(&[
        940, 945, 950, 960, 970, 980, 990
    ]));
    // A real slowdown is plausible; the median limit judges it.
    assert!(capture_difference_is_plausible(&[2_000; 7]));
    // Too few pairs to bound the median cannot prove contamination.
    assert!(capture_difference_is_plausible(&[500; 5]));
    // Every pair faster than capture-off by more than 5 percent.
    assert!(!capture_difference_is_plausible(&[
        900, 910, 920, 930, 940, 945, 949
    ]));
    assert!(!capture_difference_is_plausible(&[900; 7]));
}

#[test]
fn cadence_tail_tolerance_never_drops_below_five_ms_or_the_host_noise_floor() {
    // A quiet capture-off arm keeps the original 5 ms rule.
    let quiet_off = [10_000, 10_100, 10_200, 10_300, 10_400, 10_500, 11_000];
    let tail = |last| [10_000, 10_100, 10_200, 10_300, 10_400, 10_500, last];
    assert!(window_tail_within_limit(&quiet_off, &tail(16_000)));
    assert!(!window_tail_within_limit(&quiet_off, &tail(16_001)));
    // A 19.7 ms host spike in capture-off itself: a 5 ms difference cannot
    // be resolved below that floor, so the tolerance is the floor.
    let spiked_off = tail(30_000);
    assert!(window_tail_within_limit(&spiked_off, &tail(49_700)));
    assert!(!window_tail_within_limit(&spiked_off, &tail(49_701)));

    let mut pool = CadencePool::default();
    assert!(!pool.tail_within_limit());
    let stalled = tail(40_000);
    pool.add_window(&quiet_off, &stalled);
    pool.add_window(&spiked_off, &stalled);
    assert!(!pool.tail_resolved());
    pool.add_window(&quiet_off, &quiet_off);
    assert!(pool.tail_resolved());
    // One spike among three judged windows is noise.
    assert_eq!((pool.tail_windows, pool.tail_violations), (3, 1));
    assert!(pool.tail_within_limit());
    // A stall that recurs in most windows is a regression.
    pool.add_window(&quiet_off, &stalled);
    pool.add_window(&quiet_off, &stalled);
    assert_eq!((pool.tail_windows, pool.tail_violations), (5, 3));
    assert!(!pool.tail_within_limit());
}

#[test]
fn cadence_verdict_decides_only_outside_the_noise_interval() {
    // Seven pairs bound the median by their extremes.
    assert_eq!(
        median_confidence_bounds(&[990, 1_000, 1_010, 1_020, 1_030, 1_040, 1_050]),
        Some((990, 1_050))
    );
    // Fourteen pairs drop two order statistics from each end.
    let fourteen = (0..14).map(|index| 1_000 + index).collect::<Vec<_>>();
    assert_eq!(median_confidence_bounds(&fourteen), Some((1_002, 1_011)));
    assert_eq!(median_confidence_bounds(&[1_000; 5]), None);

    let clean = [980, 990, 1_000, 1_010, 1_020, 1_030, 1_040];
    assert_eq!(cadence_verdict(&clean, false), CadenceVerdict::WithinLimit);
    assert_eq!(
        cadence_verdict(&[900; 7], false),
        CadenceVerdict::Implausible
    );
    let regressed = [1_080, 1_090, 1_100, 1_110, 1_120, 1_130, 1_140];
    assert_eq!(
        cadence_verdict(&regressed, false),
        CadenceVerdict::OverLimit
    );
    // A noisy window whose median is within the limit is not yet a pass,
    // and a noisy window over the limit is not yet a failure.
    let noisy_within = [900, 950, 1_000, 1_010, 1_020, 1_100, 1_300];
    assert_eq!(
        cadence_verdict(&noisy_within, false),
        CadenceVerdict::Undecided
    );
    let noisy_over = [900, 1_000, 1_060, 1_080, 1_100, 1_200, 1_300];
    assert_eq!(
        cadence_verdict(&noisy_over, false),
        CadenceVerdict::Undecided
    );
    // After the last window the pooled median decides, so a real overhead
    // above the limit still fails however noisy the host was.
    assert_eq!(
        cadence_verdict(&noisy_within, true),
        CadenceVerdict::WithinLimit
    );
    assert_eq!(
        cadence_verdict(&noisy_over, true),
        CadenceVerdict::OverLimit
    );
}
