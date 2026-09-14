//! Pivot detection. A pivot at `index` can only be KNOWN at `index + right`;
//! reading one earlier is seeing the future.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pivot {
    pub index: usize,
    pub value: f64,
    pub confirmed_at: usize,
}

pub fn pivot_highs(src: &[Option<f64>], left: usize, right: usize) -> Vec<Pivot> {
    find(src, left, right, true)
}

pub fn pivot_lows(src: &[Option<f64>], left: usize, right: usize) -> Vec<Pivot> {
    find(src, left, right, false)
}

/// Strict on both sides: the centre must beat every neighbour, so a tie is not a
/// pivot.
fn find(src: &[Option<f64>], left: usize, right: usize, want_high: bool) -> Vec<Pivot> {
    assert!(left >= 1 && right >= 1, "pivot sides must be >= 1");
    let mut out = Vec::new();
    if src.len() <= right {
        return out;
    }
    for i in left..src.len() - right {
        let Some(center) = src[i] else { continue };
        let is_pivot = (i - left..=i + right).filter(|&j| j != i).all(|j| match src[j] {
            None => false,
            Some(v) => if want_high { v < center } else { v > center },
        });
        if is_pivot {
            out.push(Pivot { index: i, value: center, confirmed_at: i + right });
        }
    }
    out
}
