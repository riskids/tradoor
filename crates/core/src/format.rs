//! Price precision and formatting for axis labels and readouts.
//!
//! Crypto prices span 0.00000285 to 63,749 in one list, so precision is read off
//! the instrument's own traded prices rather than pinned.

pub const MAX_PRICE_DECIMALS: usize = 8;

/// Decimals a number carries in its shortest round-trip form. Rust never prints
/// f64 in exponent notation, so `1.2e-7` reads as `0.00000012` → 8.
pub fn decimals_of(v: f64) -> usize {
    if !v.is_finite() {
        return 0;
    }
    let s = v.abs().to_string();
    s.find('.').map_or(0, |dot| s.len() - dot - 1)
}

/// The precision an instrument trades at, from PRICES ONLY — an indicator carries
/// seventeen decimals of float noise and would push everything to the cap. Two
/// decimals is the floor.
pub fn price_precision(values: impl IntoIterator<Item = f64>) -> usize {
    let mut digits = 2;
    for v in values {
        if v.is_finite() {
            digits = digits.max(decimals_of(v));
            if digits >= MAX_PRICE_DECIMALS {
                return MAX_PRICE_DECIMALS;
            }
        }
    }
    digits
}

fn bucket(abs: f64) -> usize {
    match abs {
        a if a == 0.0 || a >= 1000.0 => 2,
        a if a >= 1.0 => 4,
        a if a >= 0.01 => 5,
        a if a >= 0.0001 => 6,
        _ => MAX_PRICE_DECIMALS,
    }
}

/// Decimals for one quoted price when the instrument's precision is unknown.
pub fn price_decimals(v: f64) -> usize {
    if !v.is_finite() {
        return 2;
    }
    MAX_PRICE_DECIMALS.min(bucket(v.abs()).max(decimals_of(v)))
}

/// `62,685.10`. `None` digits falls back to `price_decimals`, which is only safe
/// for a traded price.
pub fn format_price(v: f64, digits: Option<usize>) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    let d = digits.unwrap_or_else(|| price_decimals(v));
    let fixed = format!("{:.*}", d, v.abs());
    let (int, frac) = fixed.split_once('.').map_or((fixed.as_str(), None), |(i, f)| (i, Some(f)));
    let mut grouped = String::with_capacity(fixed.len() + int.len() / 3 + 1);
    if v < 0.0 {
        grouped.push('-');
    }
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if let Some(frac) = frac {
        grouped.push('.');
        grouped.push_str(frac);
    }
    grouped
}

/// `27.4M`, `1.2B`.
pub fn format_turnover(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    match v.abs() {
        a if a >= 1e12 => format!("{:.1}T", v / 1e12),
        a if a >= 1e9 => format!("{:.1}B", v / 1e9),
        a if a >= 1e6 => format!("{:.1}M", v / 1e6),
        a if a >= 1e3 => format!("{:.0}K", v / 1e3),
        _ => format!("{v:.0}"),
    }
}

/// A large figure to four significant-ish digits: `5.327M`, `106.2K`, `12.35`.
pub fn format_compact(v: f64) -> String {
    if !v.is_finite() {
        return "—".to_string();
    }
    let (scaled, unit) = match v.abs() {
        a if a >= 1e12 => (v / 1e12, "T"),
        a if a >= 1e9 => (v / 1e9, "B"),
        a if a >= 1e6 => (v / 1e6, "M"),
        a if a >= 1e3 => (v / 1e3, "K"),
        _ => return format!("{v:.2}"),
    };
    let decimals = if scaled.abs() >= 100.0 { 1 } else if scaled.abs() >= 10.0 { 2 } else { 3 };
    format!("{scaled:.decimals$}{unit}")
}

/// Time left until a bar closes: `04:12`, `2:05:33`, or `3d 04h`.
pub fn format_countdown(ms: i64) -> String {
    if ms <= 0 {
        return "00:00".to_string();
    }
    let total = ms / 1000;
    let (days, hours, minutes, seconds) = (total / 86_400, total % 86_400 / 3600, total % 3600 / 60, total % 60);
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn compact_figures() {
        assert_eq!(super::format_compact(5_327_000.0), "5.327M");
        assert_eq!(super::format_compact(106_188.6), "106.2K");
        assert_eq!(super::format_compact(12_345_678_901.0), "12.35B");
        assert_eq!(super::format_compact(12.345), "12.35");
    }

    use super::*;

    #[test]
    fn decimals_see_through_exponents() {
        assert_eq!((decimals_of(62685.12), decimals_of(0.2479), decimals_of(0.0062015), decimals_of(100.0)), (2, 4, 7, 0));
        assert_eq!((decimals_of(0.00000012), decimals_of(1e-7), decimals_of(2.85e-6)), (8, 7, 8));
    }

    #[test]
    fn precision_floors_caps_and_skips_gaps() {
        assert_eq!(price_precision([0.24, 0.2479, 0.248]), 4);
        assert_eq!(price_precision([100.0, 200.0]), 2);
        assert_eq!(price_precision([0.2612345678901234]), MAX_PRICE_DECIMALS);
        assert_eq!(price_precision([f64::NAN, 0.0062015]), 7);
    }

    #[test]
    fn prices_format_like_the_original() {
        assert_eq!(format_price(0.0062015, Some(7)), "0.0062015");
        assert_eq!(format_price(0.006, Some(7)), "0.0060000");
        assert_eq!(format_price(62685.1, Some(2)), "62,685.10");
        assert_eq!(format_price(0.2612345678901234, Some(4)), "0.2612");
        assert_eq!(format_price(0.0062015, None), "0.0062015");
        assert_eq!(format_price(0.24, None), "0.24000");
        assert_eq!(format_price(62685.1, None), "62,685.10");
        assert_eq!(format_price(0.2612345678901234, None), "0.26123457");
        assert_eq!(format_price(f64::NAN, None), "—");
        assert_eq!(format_price(-1234567.5, Some(1)), "-1,234,567.5");
    }

    #[test]
    fn countdown() {
        assert_eq!(format_countdown(12_000), "00:12");
        assert_eq!(format_countdown(72_000), "01:12");
        assert_eq!(format_countdown(59 * 60_000 + 59_000), "59:59");
        assert_eq!(format_countdown(2 * 3_600_000 + 5 * 60_000 + 33_000), "2:05:33");
        assert_eq!(format_countdown(3 * 86_400_000 + 4 * 3_600_000), "3d 04h");
        assert_eq!((format_countdown(0), format_countdown(-5_000)), ("00:00".to_string(), "00:00".to_string()));
    }
}
