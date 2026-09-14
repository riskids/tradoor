//! `ta.crossover` / `ta.crossunder`. The previous bar must hold the strictly
//! opposite relationship, so two series that touch and separate produce nothing.
//! Warm-up yields `None` — "not judgeable" is not "no cross".

pub fn cross_over(a: &[Option<f64>], b: &[Option<f64>]) -> Vec<Option<bool>> {
    detect(a, b, true)
}

pub fn cross_under(a: &[Option<f64>], b: &[Option<f64>]) -> Vec<Option<bool>> {
    detect(a, b, false)
}

fn detect(a: &[Option<f64>], b: &[Option<f64>], up: bool) -> Vec<Option<bool>> {
    let n = a.len().max(b.len());
    let at = |s: &[Option<f64>], i: usize| s.get(i).copied().flatten();
    let mut out = vec![None; n];
    for i in 1..n {
        let (Some(a_now), Some(b_now), Some(a_prev), Some(b_prev)) = (at(a, i), at(b, i), at(a, i - 1), at(b, i - 1)) else {
            continue;
        };
        out[i] = Some(if up { a_prev <= b_prev && a_now > b_now } else { a_prev >= b_prev && a_now < b_now });
    }
    out
}
