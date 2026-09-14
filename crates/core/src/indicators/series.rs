//! Null-aware series helpers: SMA, EMA, RMA, stdev, highest/lowest.

use super::Series;

fn assert_len(len: usize, who: &str) {
    assert!(len >= 1, "{who}: length must be >= 1");
}

/// `ta.sma`. A rolling sum, RESYNCED from scratch every `len` steps: without
/// that, the chain of additions and subtractions drifts, and the SMA of a
/// constant 100 becomes 100.00000000000001 — enough to push a bounded value such
/// as Stoch RSI past 100.
pub fn sma(src: &[Option<f64>], len: usize) -> Series {
    assert_len(len, "sma");
    let mut out = vec![None; src.len()];
    let (mut sum, mut run, mut since_resync) = (0.0, 0usize, 0usize);
    for i in 0..src.len() {
        let Some(v) = src[i] else {
            sum = 0.0;
            run = 0;
            since_resync = 0;
            continue;
        };
        sum += v;
        run += 1;
        if run > len {
            sum -= src[i - len].expect("inside a valid run");
            run = len;
        }
        if run < len {
            continue;
        }
        since_resync += 1;
        if since_resync >= len {
            sum = src[i + 1 - len..=i].iter().map(|v| v.expect("inside a valid run")).sum();
            since_resync = 0;
        }
        out[i] = Some(sum / len as f64);
    }
    out
}

/// Exponential smoothing seeded with the SMA of the first `len` bars — the basis
/// of both `ema` and `rma`. Seeding from the first value instead leaves EMA 200
/// off TradingView's for hundreds of bars.
pub fn exp_smooth(src: &[Option<f64>], len: usize, alpha: f64) -> Series {
    assert_len(len, "exp_smooth");
    let mut out = vec![None; src.len()];
    let (mut prev, mut sum, mut run): (Option<f64>, f64, usize) = (None, 0.0, 0);
    for i in 0..src.len() {
        let Some(v) = src[i] else {
            prev = None;
            sum = 0.0;
            run = 0;
            continue;
        };
        match prev {
            None => {
                sum += v;
                run += 1;
                if run == len {
                    let seed = sum / len as f64;
                    prev = Some(seed);
                    out[i] = Some(seed);
                }
            }
            Some(p) => {
                let next = alpha * v + (1.0 - alpha) * p;
                prev = Some(next);
                out[i] = Some(next);
            }
        }
    }
    out
}

/// `ta.ema`, alpha = 2/(len+1).
pub fn ema(src: &[Option<f64>], len: usize) -> Series {
    exp_smooth(src, len, 2.0 / (len as f64 + 1.0))
}

/// `ta.rma` — Wilder, alpha = 1/len. What RSI and ATR use; reaching for `ema`
/// here is the classic reason an RSI disagrees with TradingView.
pub fn rma(src: &[Option<f64>], len: usize) -> Series {
    exp_smooth(src, len, 1.0 / len as f64)
}

pub fn highest(src: &[Option<f64>], len: usize) -> Series {
    rolling_extreme(src, len, true)
}

pub fn lowest(src: &[Option<f64>], len: usize) -> Series {
    rolling_extreme(src, len, false)
}

fn rolling_extreme(src: &[Option<f64>], len: usize, want_max: bool) -> Series {
    assert_len(len, "highest/lowest");
    let mut out = vec![None; src.len()];
    let mut run = 0usize;
    for i in 0..src.len() {
        if src[i].is_none() {
            run = 0;
            continue;
        }
        run = (run + 1).min(len);
        if run < len {
            continue;
        }
        let mut acc = src[i].expect("checked");
        for w in src[i + 1 - len..=i].iter().map(|v| v.expect("inside a valid run")) {
            if if want_max { w > acc } else { w < acc } {
                acc = w;
            }
        }
        out[i] = Some(acc);
    }
    out
}

/// Population standard deviation (÷N) — Pine's default, and what the built-in
/// Bollinger Bands use.
pub fn stdev(src: &[Option<f64>], len: usize) -> Series {
    assert_len(len, "stdev");
    let basis = sma(src, len);
    let mut out = vec![None; src.len()];
    for i in 0..src.len() {
        let Some(mean) = basis[i] else { continue };
        let acc: f64 = src[i + 1 - len..=i]
            .iter()
            .map(|v| {
                let d = v.expect("inside a valid window") - mean;
                d * d
            })
            .sum();
        out[i] = Some((acc / len as f64).sqrt());
    }
    out
}

/// `ta.change`.
pub fn change(src: &[Option<f64>], offset: usize) -> Series {
    let mut out = vec![None; src.len()];
    for i in offset..src.len() {
        if let (Some(cur), Some(prev)) = (src[i], src[i - offset]) {
            out[i] = Some(cur - prev);
        }
    }
    out
}

/// Element-wise over two series; `None` when either side is.
pub fn combine(a: &[Option<f64>], b: &[Option<f64>], f: impl Fn(f64, f64) -> f64) -> Series {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| match (a.get(i).copied().flatten(), b.get(i).copied().flatten()) {
            (Some(x), Some(y)) => Some(f(x, y)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{assert_close, some, synthetic_closes, warmup_length, EPS};

    fn naive_sma(src: &[f64], len: usize) -> Series {
        (0..src.len())
            .map(|i| (i + 1 >= len).then(|| src[i + 1 - len..=i].iter().sum::<f64>() / len as f64))
            .collect()
    }

    #[test]
    fn sma_hand_computed() {
        assert_eq!(sma(&some(&[1.0, 2.0, 3.0, 4.0, 5.0]), 3), vec![None, None, Some(2.0), Some(3.0), Some(4.0)]);
        assert_eq!(warmup_length(&sma(&some(&synthetic_closes(50, 100.0)), 20)), 19);
    }

    #[test]
    fn sma_matches_naive() {
        let closes = synthetic_closes(400, 100.0);
        for len in [2, 5, 14, 20, 50, 200] {
            let (fast, slow) = (sma(&some(&closes), len), naive_sma(&closes, len));
            for i in 0..closes.len() {
                match slow[i] {
                    None => assert_eq!(fast[i], None),
                    Some(v) => assert_close(fast[i], v, 1e-9),
                }
            }
        }
    }

    #[test]
    fn a_null_voids_the_window() {
        let out = sma(&[Some(1.0), Some(2.0), None, Some(4.0), Some(5.0), Some(6.0), Some(7.0)], 3);
        assert_eq!(&out[..5], &[None; 5]);
        assert_close(out[5], 5.0, EPS);
        assert_close(out[6], 6.0, EPS);
    }

    #[test]
    fn the_rolling_sum_does_not_drift() {
        let flat = vec![Some(100.0); 2000];
        for len in [3, 14, 20, 200] {
            assert!(sma(&flat, len).into_iter().flatten().all(|v| v == 100.0), "sma({len}) drifted");
        }
        let src: Vec<f64> = (0..1000).map(|i| 100_000.0 + (i % 7) as f64 * 0.01).collect();
        let (rolled, naive) = (sma(&some(&src), 20), naive_sma(&src, 20));
        for i in 19..src.len() {
            assert_close(rolled[i], naive[i].unwrap(), 1e-10);
        }
    }

    #[test]
    fn ema_is_seeded_with_an_sma() {
        assert_eq!(ema(&some(&[1.0, 2.0, 3.0, 10.0, 10.0]), 3), vec![None, None, Some(2.0), Some(6.0), Some(8.0)]);
        let closes = some(&synthetic_closes(500, 100.0));
        assert_eq!(warmup_length(&ema(&closes, 200)), 199);
        for len in [12, 25, 50, 200] {
            assert_close(ema(&closes, len)[len - 1], sma(&closes, len)[len - 1].unwrap(), 1e-9);
        }
    }

    #[test]
    fn rma_uses_wilder_alpha() {
        let out = rma(&some(&[1.0, 2.0, 3.0, 10.0, 10.0]), 3);
        assert_eq!(&out[..2], &[None, None]);
        assert_close(out[2], 2.0, EPS);
        assert_close(out[3], 4.666666666666667, EPS);
        assert_close(out[4], 6.444444444444445, EPS);
    }

    #[test]
    fn stdev_is_population() {
        let out = stdev(&some(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]), 8);
        assert_close(out[7], 2.0, EPS);
    }

    #[test]
    fn highest_lowest_and_change() {
        let src = some(&[5.0, 3.0, 8.0, 1.0, 9.0, 2.0]);
        assert_eq!(&highest(&src, 3)[2..], &[Some(8.0), Some(8.0), Some(9.0), Some(9.0)]);
        assert_eq!(&lowest(&src, 3)[2..], &[Some(3.0), Some(1.0), Some(1.0), Some(1.0)]);
        assert_eq!(change(&some(&[10.0, 11.0, 10.5]), 1), vec![None, Some(1.0), Some(-0.5)]);
    }
}
